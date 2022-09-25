//! Static module image summary.

use crate::value::WasmVal;
use std::collections::BTreeMap;
use walrus::{
    ActiveData, ActiveDataLocation, DataKind, GlobalId, GlobalKind, InitExpr, Memory, MemoryId,
    Module,
};

#[derive(Clone, Debug)]
pub struct Image {
    pub memories: BTreeMap<MemoryId, MemImage>,
    pub globals: BTreeMap<GlobalId, WasmVal>,
    pub stack_pointer: Option<GlobalId>,
    pub main_heap: Option<MemoryId>,
}

#[derive(Clone, Debug)]
pub struct MemImage {
    pub image: Vec<u8>,
    pub len: usize,
}

pub fn build_image(module: &Module) -> anyhow::Result<Image> {
    Ok(Image {
        memories: module
            .memories
            .iter()
            .flat_map(|mem| maybe_mem_image(module, mem).map(|image| (mem.id(), image)))
            .collect(),
        globals: module
            .globals
            .iter()
            .flat_map(|g| match &g.kind {
                GlobalKind::Local(InitExpr::Value(val)) => Some((g.id(), WasmVal::from(*val))),
                _ => None,
            })
            .collect(),
        // HACK: assume first global is shadow stack pointer.
        stack_pointer: module.globals.iter().next().map(|g| g.id()),
        // HACK: assume first memory is main heap.
        main_heap: module.memories.iter().next().map(|m| m.id()),
    })
}

fn maybe_mem_image(module: &Module, mem: &Memory) -> Option<MemImage> {
    const WASM_PAGE: usize = 1 << 16;
    let len = (mem.initial as usize) * WASM_PAGE;
    let mut image = vec![0; len];

    for &segment_id in &mem.data_segments {
        let segment = module.data.get(segment_id);
        match segment.kind {
            DataKind::Passive => continue,
            DataKind::Active(ActiveData {
                memory,
                location: ActiveDataLocation::Relative(..),
            }) => {
                return None;
            }
            DataKind::Active(ActiveData {
                memory,
                location: ActiveDataLocation::Absolute(offset),
            }) => {
                let offset = offset as usize;
                image[offset..(offset + segment.value.len())].copy_from_slice(&segment.value[..]);
            }
        }
    }

    Some(MemImage { image, len })
}

pub fn update(module: &mut Module, im: &Image) {
    for (mem_id, mem) in &im.memories {
        for data_id in &module.memories.get(*mem_id).data_segments {
            module.data.delete(*data_id);
        }
        module.data.add(
            DataKind::Active(ActiveData {
                memory: *mem_id,
                location: ActiveDataLocation::Absolute(0),
            }),
            mem.image.clone(),
        );
    }
}