#![allow(dead_code)]

use std::path::PathBuf;
use structopt::StructOpt;

mod constant_offsets;
mod dce;
mod directive;
mod escape;
mod eval;
mod filter;
mod image;
mod intrinsics;
mod liveness;
mod state;
mod stats;
mod value;

const STUBS: &'static str = include_str!("../lib/weval-stubs.wat");

#[derive(Clone, Debug, StructOpt)]
pub enum Command {
    /// Partially evaluate a Wasm module, optionally wizening first.
    Weval {
        /// The input Wasm module.
        #[structopt(short = "i")]
        input_module: PathBuf,

        /// The output Wasm module.
        #[structopt(short = "o")]
        output_module: PathBuf,

        /// Whether to Wizen the module first.
        #[structopt(short = "w")]
        wizen: bool,

        /// Show stats on specialization code size.
        #[structopt(long = "show-stats")]
        show_stats: bool,

        /// Output IR for generic and specialized functions to files in a directory.
        #[structopt(long = "output-ir")]
        output_ir: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let _ = env_logger::try_init();
    let cmd = Command::from_args();

    match cmd {
        Command::Weval {
            input_module,
            output_module,
            wizen,
            show_stats,
            output_ir,
        } => weval(input_module, output_module, wizen, show_stats, output_ir),
    }
}

fn wizen(raw_bytes: Vec<u8>) -> anyhow::Result<Vec<u8>> {
    let mut w = wizer::Wizer::new();
    w.allow_wasi(true)?;
    w.inherit_env(true);
    w.dir(".");
    w.wasm_bulk_memory(true);
    w.preload_bytes("weval", STUBS.as_bytes().to_vec())?;
    w.func_rename("_start", "wizer.resume");
    w.run(&raw_bytes[..])
}

fn weval(
    input_module: PathBuf,
    output_module: PathBuf,
    do_wizen: bool,
    show_stats: bool,
    output_ir: Option<PathBuf>,
) -> anyhow::Result<()> {
    let raw_bytes = std::fs::read(&input_module)?;

    // Optionally, Wizen the module first.
    let module_bytes = if do_wizen {
        wizen(raw_bytes)?
    } else {
        raw_bytes
    };

    // Load module.
    let mut frontend_opts = waffle::FrontendOptions::default();
    frontend_opts.debug = true;
    let module = waffle::Module::from_wasm_bytes(&module_bytes[..], &frontend_opts)?;

    // Build module image.
    let mut im = image::build_image(&module, None)?;

    // Collect directives.
    let directives = directive::collect(&module, &mut im)?;
    log::debug!("Directives: {:?}", directives);

    // Make sure IR output directory exists.
    if let Some(dir) = &output_ir {
        std::fs::create_dir_all(dir)?;
    }

    // Partially evaluate.
    let progress = indicatif::ProgressBar::new(0);
    let mut result =
        eval::partially_evaluate(module, &mut im, &directives[..], Some(progress), output_ir)?;

    // Update memories in module.
    image::update(&mut result.module, &im);

    log::debug!("Final module:\n{}", result.module.display());

    if show_stats {
        for stats in result.stats {
            eprintln!(
                "Function {}: {} blocks, {} insts)",
                stats.generic, stats.generic_blocks, stats.generic_insts,
            );
            eprintln!(
                "   specialized ({} times): {} blocks, {} insts",
                stats.specializations, stats.specialized_blocks, stats.specialized_insts
            );
            eprintln!(
                "   virtstack: {} reads ({} mem), {} writes ({} mem)",
                stats.virtstack_reads,
                stats.virtstack_reads_mem,
                stats.virtstack_writes,
                stats.virtstack_writes_mem
            );
            eprintln!(
                "   locals: {} reads ({} mem), {} writes ({} mem)",
                stats.local_reads,
                stats.local_reads_mem,
                stats.local_writes,
                stats.local_writes_mem
            );
            eprintln!(
                "   live values at block starts: {} ({} per block)",
                stats.live_value_at_block_start,
                (stats.live_value_at_block_start as f64) / (stats.specialized_blocks as f64),
            );
        }
    }

    let bytes = result.module.to_wasm_bytes()?;

    let bytes = filter::filter(&bytes[..])?;

    std::fs::write(&output_module, &bytes[..])?;

    Ok(())
}
