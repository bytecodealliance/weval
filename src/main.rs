#![allow(dead_code)]

use std::path::PathBuf;
use structopt::StructOpt;

use wasmtime::*;
use wasmtime_wasi::p1;

mod cache;
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

        /// Preopened directories during Wizening, if any.
        #[structopt(long = "dir")]
        preopens: Vec<PathBuf>,

        /// Name of the Wizer initialization function to call.
        #[structopt(long = "init-func", default_value = "wizer-initialize")]
        init_func: String,

        /// Cache file to use.
        #[structopt(long = "cache")]
        cache: Option<PathBuf>,

        /// Read-only cache file to query.
        #[structopt(long = "cache-ro")]
        cache_ro: Option<PathBuf>,

        /// Show stats on specialization code size.
        #[structopt(long = "show-stats")]
        show_stats: bool,

        /// Output IR for generic and specialized functions to files in a directory.
        #[structopt(long = "output-ir")]
        output_ir: Option<PathBuf>,

        /// Emit verbose progress messages.
        #[structopt(short = "v", long = "verbose")]
        verbose: bool,
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
            preopens,
            init_func,
            cache,
            cache_ro,
            show_stats,
            output_ir,
            verbose,
        } => weval(
            input_module,
            output_module,
            wizen,
            preopens,
            init_func,
            cache,
            cache_ro,
            show_stats,
            output_ir,
            verbose,
        ),
    }
}

fn wizen(raw_bytes: Vec<u8>, preopens: Vec<PathBuf>, init_func: String) -> anyhow::Result<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(wizen_impl(raw_bytes, preopens, init_func))
}

async fn wizen_impl(
    raw_bytes: Vec<u8>,
    preopens: Vec<PathBuf>,
    init_func: String,
) -> anyhow::Result<Vec<u8>> {
    let mut config = Config::new();
    config.async_support(true);
    config.wasm_bulk_memory(true);
    let engine = Engine::new(&config)?;

    let mut wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new();
    wasi_ctx.inherit_stdio();
    wasi_ctx.inherit_env();
    for preopen in &preopens {
        wasi_ctx.preopened_dir(
            preopen,
            preopen.to_str().unwrap_or("."),
            wasmtime_wasi::DirPerms::all(),
            wasmtime_wasi::FilePerms::all(),
        )?;
    }

    let mut store = Store::new(&engine, wasi_ctx.build_p1());
    let mut linker = Linker::new(&engine);
    p1::add_to_linker_async(&mut linker, |cx| cx)?;

    // Preload the weval stubs module.
    let stubs_module = Module::new(&engine, STUBS)?;
    let stubs_instance = linker.instantiate_async(&mut store, &stubs_module).await?;
    linker.instance(&mut store, "weval", stubs_instance)?;

    let mut wizer = wasmtime_wizer::Wizer::new();
    wizer.init_func(&init_func);
    wizer.func_rename("_start", "wizer.resume");

    wizer
        .run(&mut store, &raw_bytes, async |store, module| {
            linker.define_unknown_imports_as_traps(module)?;
            linker.instantiate_async(store, module).await
        })
        .await
}

/// Weval a wasm.
pub fn weval(
    input_module: PathBuf,
    output_module: PathBuf,
    do_wizen: bool,
    preopens: Vec<PathBuf>,
    init_func: String,
    cache: Option<PathBuf>,
    cache_ro: Option<PathBuf>,
    show_stats: bool,
    output_ir: Option<PathBuf>,
    verbose: bool,
) -> anyhow::Result<()> {
    if verbose {
        eprintln!("Reading raw module bytes...");
    }
    let raw_bytes = std::fs::read(&input_module)?;

    // Compute a hash of the original module so we can cache results
    // keyed on that hash (and weval request arg strings).
    let input_hash = cache::compute_hash(&raw_bytes[..]);

    // Open the cache and read-only cache, if any.
    let cache = cache::Cache::open(
        cache.as_ref().map(|p| p.as_path()),
        cache_ro.as_ref().map(|p| p.as_path()),
        input_hash,
    )?;

    // Optionally, Wizen the module first.
    let module_bytes = if do_wizen {
        if verbose {
            eprintln!("Wizening the module with its input...");
        }
        wizen(raw_bytes, preopens, init_func)?
    } else {
        raw_bytes
    };

    // Load module.
    if verbose {
        eprintln!("Parsing the module...");
    }
    let mut frontend_opts = waffle::FrontendOptions::default();
    frontend_opts.debug = true;
    let module = waffle::Module::from_wasm_bytes(&module_bytes[..], &frontend_opts)?;

    // Build module image.
    if verbose {
        eprintln!("Building memory image...");
    }
    let mut im = image::build_image(&module, None)?;

    // Collect directives.
    let directives = directive::collect(&module, &mut im)?;
    log::debug!("Directives: {:?}", directives);

    // Make sure IR output directory exists.
    if let Some(dir) = &output_ir {
        std::fs::create_dir_all(dir)?;
    }

    // Partially evaluate.
    if verbose {
        eprintln!("Specializing functions...");
    }
    let progress = if verbose {
        Some(indicatif::ProgressBar::new(0))
    } else {
        None
    };
    let mut result = eval::partially_evaluate(
        module,
        &mut im,
        &directives[..],
        progress,
        output_ir,
        &cache,
    )?;

    // Update memories in module.
    if verbose {
        eprintln!("Updatimg memory image...");
    }
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

    if verbose {
        eprintln!("Serializing back to binary form...");
    }
    let bytes = result.module.to_wasm_bytes()?;

    if verbose {
        eprintln!("Performing post-filter pass to remove intrinsics...");
    }
    let bytes = filter::filter(&bytes[..])?;

    if verbose {
        eprintln!("Writing output file...");
    }
    std::fs::write(&output_module, &bytes[..])?;

    if verbose {
        eprintln!("Done.");
    }
    Ok(())
}
