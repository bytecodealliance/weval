//! Static module image summary.

use crate::value::WasmVal;
use rayon::iter::{IntoParallelIterator, ParallelExtend, ParallelIterator};
use std::collections::BTreeMap;
use std::ops::Range;
use waffle::{Func, Global, Memory, MemoryData, MemorySegment, Module, Table, WASM_PAGE};

/// The maximum number of data segments that we will emit. Most
/// engines support more than this, but we want to leave some
/// headroom.
const MAX_DATA_SEGMENTS: usize = 10_000;

/// The minimum overhead of defining a new active data segment: one for the
/// memory index LEB, two for the memory offset init expression (one for the
/// `i32.const` opcode and another for the constant immediate LEB), and finally
/// one for the data length LEB.
// NOTE: kept as module-level constant rather than local (as in wizer) because
// it is used in tests.
const MIN_ACTIVE_SEGMENT_OVERHEAD: usize = 4;

#[derive(Clone, Debug)]
pub(crate) struct Image {
    pub memories: BTreeMap<Memory, MemImage>,
    pub globals: BTreeMap<Global, WasmVal>,
    pub tables: BTreeMap<Table, Vec<Func>>,
    pub stack_pointer: Option<Global>,
    pub main_heap: Option<Memory>,
    pub main_table: Option<Table>,
}

#[derive(Clone, Debug)]
pub(crate) struct MemImage {
    pub image: Vec<u8>,
}

impl MemImage {
    pub fn len(&self) -> usize {
        self.image.len()
    }
}

#[derive(Clone, Debug)]
struct DataSegmentRange {
    memory_index: Memory,
    range: Range<usize>,
}

impl DataSegmentRange {
    /// What is the gap between two consecutive data segments?
    ///
    /// `self` must be in front of `other` and they must not overlap with each
    /// other.
    fn gap(&self, other: &Self) -> usize {
        debug_assert_eq!(self.memory_index, other.memory_index);
        debug_assert!(self.range.end <= other.range.start);
        other.range.start - self.range.end
    }

    /// Merge two consecutive data segments.
    ///
    /// `self` must be in front of `other` and they must not overlap with each
    /// other.
    fn merge(&mut self, other: &Self) {
        debug_assert_eq!(self.memory_index, other.memory_index);
        debug_assert!(self.range.end <= other.range.start);
        self.range.end = other.range.end;
    }
}

pub(crate) fn build_image(module: &Module, snapshot_bytes: Option<&[u8]>) -> anyhow::Result<Image> {
    Ok(Image {
        memories: module
            .memories
            .entries()
            .flat_map(|(id, mem)| maybe_mem_image(mem, snapshot_bytes).map(|image| (id, image)))
            .collect(),
        globals: module
            .globals
            .entries()
            .flat_map(|(global_id, data)| match data.value {
                Some(bits) => Some((global_id, WasmVal::from_bits(data.ty, bits)?)),
                _ => None,
            })
            .collect(),
        tables: module
            .tables
            .entries()
            .map(|(id, data)| (id, data.func_elements.clone().unwrap_or(vec![])))
            .collect(),
        // HACK: assume first global is shadow stack pointer.
        stack_pointer: module.globals.iter().next(),
        // HACK: assume first memory is main heap.
        main_heap: module.memories.iter().next(),
        // HACK: assume first table is used for function pointers.
        main_table: module.tables.iter().next(),
    })
}

fn maybe_mem_image(mem: &MemoryData, snapshot_bytes: Option<&[u8]>) -> Option<MemImage> {
    if let Some(b) = snapshot_bytes {
        return Some(MemImage { image: b.to_vec() });
    }

    let len = mem.initial_pages * WASM_PAGE;
    let mut image = vec![0; len];

    for segment in &mem.segments {
        image[segment.offset..(segment.offset + segment.data.len())]
            .copy_from_slice(&segment.data[..]);
    }

    Some(MemImage { image })
}

/// Both snapshot_memories and remove_excess_segments are adapted from wizer,
/// see https://github.com/bytecodealliance/wasmtime/blob/8adf03d4557acaa8384ec084946614d3f87c39ff/crates/wizer/src/snapshot.rs#L120
///
/// Find all non-zero regions across all memories, merge nearby segments, and
/// return the final set of memory segments with data extracted. Mirrors wizer's
/// `snapshot_memories` function.
fn snapshot_memories(im: &Image) -> Vec<(Memory, MemorySegment)> {
    log::debug!("Snapshotting memories");

    // Find and record non-zero regions of memory (in parallel).
    let mut data_segments: Vec<DataSegmentRange> = vec![];
    for (&memory_index, mem) in &im.memories {
        let memory_data = &mem.image[..];
        let num_wasm_pages = memory_data.len() / WASM_PAGE;

        // Consider each Wasm page in parallel. Create data segments for each
        // region of non-zero memory.
        data_segments.par_extend((0..num_wasm_pages).into_par_iter().flat_map(|i| {
            let page_end = (i + 1) * WASM_PAGE;
            let mut start = i * WASM_PAGE;
            let mut segments = vec![];
            while start < page_end {
                let nonzero = match memory_data[start..page_end]
                    .iter()
                    .position(|byte| *byte != 0)
                {
                    None => break,
                    Some(i) => i,
                };
                start += nonzero;
                let end = memory_data[start..page_end]
                    .iter()
                    .position(|byte| *byte == 0)
                    .map_or(page_end, |zero| start + zero);
                segments.push(DataSegmentRange {
                    memory_index,
                    range: start..end,
                });
                start = end;
            }
            segments
        }));
    }

    if data_segments.is_empty() {
        return Vec::new();
    }

    // Sort data segments to enforce determinism in the face of the
    // parallelism above. It is also load-bearing for multimemory, it groups all
    // segments for one memory together for the merging and extraction below.
    data_segments.sort_by_key(|s| (s.memory_index, s.range.start));

    // Merge any contiguous segments (caused by spanning a Wasm page boundary,
    // and therefore created in separate logical threads above) or pages that
    // are within four bytes of each other. Four because this is the minimum
    // overhead of defining a new active data segment: one for the memory index
    // LEB, two for the memory offset init expression (one for the `i32.const`
    // opcode and another for the constant immediate LEB), and finally one for
    // the data length LEB).
    let mut merged_data_segments = Vec::with_capacity(data_segments.len());
    merged_data_segments.push(data_segments[0].clone());
    for b in &data_segments[1..] {
        let a = merged_data_segments.last_mut().unwrap();

        // Only merge segments for the same memory.
        if a.memory_index != b.memory_index {
            merged_data_segments.push(b.clone());
            continue;
        }

        // Only merge segments if they are contiguous or if it is definitely
        // more size efficient than leaving them apart.
        let gap = a.gap(b);
        if gap > MIN_ACTIVE_SEGMENT_OVERHEAD {
            merged_data_segments.push(b.clone());
            continue;
        }

        // Okay, merge them together into `a` (so that the next iteration can
        // merge it with its predecessor) and then omit `b`!
        a.merge(b);
    }

    remove_excess_segments(&mut merged_data_segments);

    // With the final set of data segments now extract the actual data of each
    // memory, copying it into a `MemorySegment`, to return the final list of
    // segments.
    //
    // Here the memories are iterated over again and, in tandem, the
    // `merged_data_segments` list is traversed to extract a `MemorySegment` for
    // each range that `merged_data_segments` indicates. This relies on
    // `merged_data_segments` being a sorted list by `memory_index` at least.
    let mut final_data_segments = Vec::with_capacity(merged_data_segments.len());
    let mut merged = merged_data_segments.iter().peekable();
    for (&memory_index, mem) in &im.memories {
        let memory_data = &mem.image[..];
        while let Some(segment) = merged.next_if(|s| s.memory_index == memory_index) {
            final_data_segments.push((
                memory_index,
                MemorySegment {
                    offset: segment.range.start,
                    data: memory_data[segment.range.clone()].to_vec(),
                },
            ));
        }
    }
    assert!(merged.next().is_none());

    final_data_segments
}

/// Engines apply a limit on how many segments a module may contain, and we
/// can run afoul of it. When that happens, we need to merge data segments
/// together until our number of data segments fits within the limit.
fn remove_excess_segments(merged_data_segments: &mut Vec<DataSegmentRange>) {
    if merged_data_segments.len() < MAX_DATA_SEGMENTS {
        return;
    }

    // We need to remove `excess` number of data segments.
    let excess = merged_data_segments.len() - MAX_DATA_SEGMENTS;

    #[derive(Clone, Copy, PartialEq, Eq)]
    struct GapIndex {
        gap: u32,
        // Use a `u32` instead of `usize` to fit `GapIndex` within a word on
        // 64-bit systems, using less memory.
        index: u32,
    }

    // Find the gaps between the start of one segment and the next (if they are
    // both in the same memory). We will merge the `excess` segments with the
    // smallest gaps together. Because they are the smallest gaps, this will
    // bloat the size of our data segment the least.
    let mut smallest_gaps = Vec::with_capacity(merged_data_segments.len() - 1);
    for (index, w) in merged_data_segments.windows(2).enumerate() {
        if w[0].memory_index != w[1].memory_index {
            continue;
        }
        let gap = match u32::try_from(w[0].gap(&w[1])) {
            Ok(gap) => gap,
            // If the gap is larger than 4G then don't consider these two data
            // segments for merging and assume there's enough other data
            // segments close enough together to still consider for merging to
            // get under the limit.
            Err(_) => continue,
        };
        let index = u32::try_from(index).unwrap();
        smallest_gaps.push(GapIndex { gap, index });
    }
    smallest_gaps.sort_unstable_by_key(|g| g.gap);
    smallest_gaps.truncate(excess);

    // Now merge the chosen segments together in reverse index order so that
    // merging two segments doesn't mess up the index of the next segments we
    // will to merge.
    smallest_gaps.sort_unstable_by(|a, b| a.index.cmp(&b.index).reverse());
    for GapIndex { index, .. } in smallest_gaps {
        let index = usize::try_from(index).unwrap();
        let [a, b] = merged_data_segments
            .get_disjoint_mut([index, index + 1])
            .unwrap();
        a.merge(b);

        // Okay to use `swap_remove` here because, even though it makes
        // `merged_data_segments` unsorted, the segments are still sorted within
        // the range `0..index` and future iterations will only operate within
        // that subregion because we are iterating over largest to smallest
        // indices.
        merged_data_segments.swap_remove(index + 1);
    }

    // Finally, sort the data segments again so that our output is
    // deterministic.
    merged_data_segments.sort_by_key(|s| (s.memory_index, s.range.start));
}

pub(crate) fn update(module: &mut Module, im: &Image) {
    let final_data_segments = snapshot_memories(im);

    // Clear existing segments for all memories.
    for &mem_id in im.memories.keys() {
        module.memories[mem_id].segments.clear();
    }

    // Apply the snapshotted data segments.
    for (memory_index, segment) in final_data_segments {
        module.memories[memory_index].segments.push(segment);
    }

    // Update initial_pages for all memories.
    for (&mem_id, mem) in &im.memories {
        let image_pages = mem.image.len() / WASM_PAGE;
        module.memories[mem_id].initial_pages =
            std::cmp::max(module.memories[mem_id].initial_pages, image_pages);
    }
}

impl Image {
    pub(crate) fn can_read(&self, memory: Memory, addr: u32, size: u32) -> bool {
        let end = match addr.checked_add(size) {
            Some(end) => end,
            None => return false,
        };
        let image = match self.memories.get(&memory) {
            Some(image) => image,
            None => return false,
        };
        (end as usize) <= image.len()
    }

    pub(crate) fn main_heap(&self) -> anyhow::Result<Memory> {
        self.main_heap
            .ok_or_else(|| anyhow::anyhow!("no main heap"))
    }

    pub(crate) fn read_slice(&self, id: Memory, addr: u32, len: u32) -> anyhow::Result<&[u8]> {
        let image = self.memories.get(&id).unwrap();
        let addr = usize::try_from(addr).unwrap();
        let len = usize::try_from(len).unwrap();
        if addr + len >= image.len() {
            anyhow::bail!("Out of bounds");
        }
        Ok(&image.image[addr..(addr + len)])
    }

    pub(crate) fn read_u8(&self, id: Memory, addr: u32) -> anyhow::Result<u8> {
        let image = self.memories.get(&id).unwrap();
        image
            .image
            .get(addr as usize)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Out of bounds"))
    }

    pub(crate) fn read_u16(&self, id: Memory, addr: u32) -> anyhow::Result<u16> {
        let image = self.memories.get(&id).unwrap();
        let addr = addr as usize;
        if (addr + 2) > image.len() {
            anyhow::bail!("Out of bounds");
        }
        let slice = &image.image[addr..(addr + 2)];
        Ok(u16::from_le_bytes([slice[0], slice[1]]))
    }

    pub(crate) fn read_u32(&self, id: Memory, addr: u32) -> anyhow::Result<u32> {
        let image = self.memories.get(&id).unwrap();
        let addr = addr as usize;
        if (addr + 4) > image.len() {
            anyhow::bail!("Out of bounds");
        }
        let slice = &image.image[addr..(addr + 4)];
        Ok(u32::from_le_bytes([slice[0], slice[1], slice[2], slice[3]]))
    }

    pub(crate) fn read_u64(&self, id: Memory, addr: u32) -> anyhow::Result<u64> {
        let low = self.read_u32(id, addr)?;
        let high = self.read_u32(id, addr + 4)?;
        Ok((high as u64) << 32 | (low as u64))
    }

    pub(crate) fn read_u128(&self, id: Memory, addr: u32) -> anyhow::Result<u128> {
        let low = self.read_u64(id, addr)?;
        let high = self.read_u64(id, addr + 8)?;
        Ok((high as u128) << 64 | (low as u128))
    }

    pub(crate) fn read_size(&self, id: Memory, addr: u32, size: u8) -> anyhow::Result<u64> {
        match size {
            1 => self.read_u8(id, addr).map(|x| x as u64),
            2 => self.read_u16(id, addr).map(|x| x as u64),
            4 => self.read_u32(id, addr).map(|x| x as u64),
            8 => self.read_u64(id, addr),
            _ => panic!("bad size"),
        }
    }

    pub(crate) fn read_str(&self, id: Memory, mut addr: u32) -> anyhow::Result<String> {
        let mut bytes = vec![];
        loop {
            let byte = self.read_u8(id, addr)?;
            if byte == 0 {
                break;
            }
            bytes.push(byte);
            addr += 1;
        }
        Ok(std::str::from_utf8(&bytes[..])?.to_owned())
    }

    pub(crate) fn write_u8(&mut self, id: Memory, addr: u32, value: u8) -> anyhow::Result<()> {
        let image = self.memories.get_mut(&id).unwrap();
        *image
            .image
            .get_mut(addr as usize)
            .ok_or_else(|| anyhow::anyhow!("Out of bounds"))? = value;
        Ok(())
    }

    pub(crate) fn write_u32(&mut self, id: Memory, addr: u32, value: u32) -> anyhow::Result<()> {
        let image = self.memories.get_mut(&id).unwrap();
        let addr = addr as usize;
        if (addr + 4) > image.len() {
            anyhow::bail!("Out of bounds");
        }
        let slice = &mut image.image[addr..(addr + 4)];
        slice.copy_from_slice(&value.to_le_bytes()[..]);
        Ok(())
    }

    pub(crate) fn func_ptr(&self, idx: u32) -> anyhow::Result<Func> {
        let table = self
            .main_table
            .ok_or_else(|| anyhow::anyhow!("no main table"))?;
        Ok(self
            .tables
            .get(&table)
            .unwrap()
            .get(idx as usize)
            .copied()
            .ok_or_else(|| anyhow::anyhow!("func ptr out of bounds"))?)
    }

    pub(crate) fn append_data(&mut self, id: Memory, data: Vec<u8>) {
        let image = self.memories.get_mut(&id).unwrap();
        let orig_len = image.len();
        let data_len = data.len();
        let padded_len = (data_len + WASM_PAGE - 1) & !(WASM_PAGE - 1);
        let padding = padded_len - data_len;
        image
            .image
            .extend(data.into_iter().chain(std::iter::repeat(0).take(padding)));
        log::debug!(
            "Appending data ({} bytes, {} padding): went from {} bytes to {} bytes",
            data_len,
            padding,
            orig_len,
            image.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use waffle::entity::EntityRef;

    fn image_with_mem(mem_id: Memory, data: Vec<u8>) -> Image {
        let mut memories = BTreeMap::new();
        memories.insert(mem_id, MemImage { image: data });
        Image {
            memories,
            globals: BTreeMap::new(),
            tables: BTreeMap::new(),
            stack_pointer: None,
            main_heap: None,
            main_table: None,
        }
    }

    #[test]
    fn test_snapshot_memories_basic() {
        // Place a 4-byte value at offset 1337,
        // similar to wizer basic_memory test.
        let mut data = vec![0u8; WASM_PAGE];
        let value: u32 = 0xDEADBEEF;
        data[1337..1341].copy_from_slice(&value.to_le_bytes());

        let mem = Memory::new(0);
        let im = image_with_mem(mem, data);
        let segments = snapshot_memories(&im);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].0, mem);
        assert_eq!(segments[0].1.offset, 1337);
        assert_eq!(&segments[0].1.data, &value.to_le_bytes());
    }

    #[test]
    fn test_snapshot_memories_empty() {
        let im = image_with_mem(Memory::new(0), vec![0u8; WASM_PAGE]);
        let segments = snapshot_memories(&im);
        assert!(segments.is_empty());
    }

    #[test]
    fn test_data_segment_at_end_of_memory() {
        // Place data at the last byte of a wasm page,
        // similar to wizer data_segment_at_end_of_memory test.
        let mut data = vec![0u8; WASM_PAGE];
        data[WASM_PAGE - 1] = 42;

        let im = image_with_mem(Memory::new(0), data);
        let segments = snapshot_memories(&im);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].1.offset, WASM_PAGE - 1);
        assert_eq!(segments[0].1.data, &[42]);
    }

    #[test]
    fn test_merge_contiguous_at_page_boundary() {
        // Place non-zero data spanning a wasm page boundary.
        // snapshot_memories should merge the two page boundary segments into one.
        let mut data = vec![0u8; 2 * WASM_PAGE];
        for i in (WASM_PAGE - 4)..(WASM_PAGE + 4) {
            data[i] = 0xFF;
        }

        let im = image_with_mem(Memory::new(0), data);
        let segments = snapshot_memories(&im);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].1.offset, WASM_PAGE - 4);
        assert_eq!(segments[0].1.data.len(), 8);
    }

    #[test]
    fn test_merge_close_segments() {
        // Two non-zero regions within MIN_ACTIVE_SEGMENT_OVERHEAD bytes
        // should be merged by snaphot_memories.
        let mut data = vec![0u8; WASM_PAGE];
        data[100] = 1;
        data[100 + 1 + MIN_ACTIVE_SEGMENT_OVERHEAD] = 2;

        let im = image_with_mem(Memory::new(0), data);
        let segments = snapshot_memories(&im);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn test_remove_excess_segments() {
        let m = Memory::new(0);
        let segnum = MAX_DATA_SEGMENTS + 100;
        // Each segment is 1 byte, and 10 bytes apart
        let mut segments: Vec<DataSegmentRange> = (0..segnum)
            .map(|i| DataSegmentRange {
                memory_index: m,
                range: (i * 10)..(i * 10 + 1),
            })
            .collect();

        remove_excess_segments(&mut segments);
        assert!(segments.len() <= MAX_DATA_SEGMENTS);

        // Ensure offsets are monotonically increasing
        for w in segments.windows(2) {
            assert!(w[0].range.start < w[1].range.start);
        }
    }

    #[test]
    fn test_update_produces_correct_segments() {
        // Try going throgh update
        let mut module = Module::empty();
        let mem_id = module.memories.push(waffle::MemoryData {
            initial_pages: 1,
            maximum_pages: None,
            segments: vec![],
        });

        let mut image_data = vec![0u8; WASM_PAGE];
        // Write two separated non-zero regions.
        image_data[100..104].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());
        image_data[1000..1004].copy_from_slice(&0xCAFEBABEu32.to_le_bytes());

        let im = image_with_mem(mem_id, image_data);
        update(&mut module, &im);

        // Should have 2 segments (the non-zero regions are 896 bytes apart).
        assert_eq!(module.memories[mem_id].segments.len(), 2);
        assert_eq!(module.memories[mem_id].segments[0].offset, 100);
        assert_eq!(
            module.memories[mem_id].segments[0].data,
            0xDEADBEEFu32.to_le_bytes()
        );
        assert_eq!(module.memories[mem_id].segments[1].offset, 1000);
        assert_eq!(
            module.memories[mem_id].segments[1].data,
            0xCAFEBABEu32.to_le_bytes()
        );
    }

    #[test]
    fn test_multi_memory() {
        // This mirrors wizer multi_memory test and also goes throgh update
        let mut module = Module::empty();
        let m1 = module.memories.push(waffle::MemoryData {
            initial_pages: 1,
            maximum_pages: None,
            segments: vec![],
        });
        let m2 = module.memories.push(waffle::MemoryData {
            initial_pages: 1,
            maximum_pages: None,
            segments: vec![],
        });

        let mut data1 = vec![0u8; WASM_PAGE];
        data1[1337..1341].copy_from_slice(&0xDEADBEEFu32.to_le_bytes());

        let mut data2 = vec![0u8; WASM_PAGE];
        data2[1337..1341].copy_from_slice(&0xCAFEBABEu32.to_le_bytes());

        let mut memories = BTreeMap::new();
        memories.insert(m1, MemImage { image: data1 });
        memories.insert(m2, MemImage { image: data2 });
        let im = Image {
            memories,
            globals: BTreeMap::new(),
            tables: BTreeMap::new(),
            stack_pointer: None,
            main_heap: None,
            main_table: None,
        };

        update(&mut module, &im);

        // Each memory should have its own segment at offset 1337.
        assert_eq!(module.memories[m1].segments.len(), 1);
        assert_eq!(module.memories[m1].segments[0].offset, 1337);
        assert_eq!(
            module.memories[m1].segments[0].data,
            0xDEADBEEFu32.to_le_bytes()
        );

        assert_eq!(module.memories[m2].segments.len(), 1);
        assert_eq!(module.memories[m2].segments[0].offset, 1337);
        assert_eq!(
            module.memories[m2].segments[0].data,
            0xCAFEBABEu32.to_le_bytes()
        );
    }
}
