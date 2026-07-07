use std::path::PathBuf;

use wasmtime::{Config, Engine, Linker, Module, Store};
use wasmtime_wasi::p1;

use crate::{cache, configure_wasi, directive, eval, filter, image};

const STUBS: &str = include_str!("../lib/weval-stubs.wat");

pub(crate) struct Opts {
    pub(crate) cache: Option<PathBuf>,
    pub(crate) cache_ro: Option<PathBuf>,
    pub(crate) module_hash: cache::ModuleHash,
    pub(crate) show_stats: bool,
    pub(crate) output_ir: Option<PathBuf>,
    pub(crate) verbose: bool,
}

pub(crate) async fn wizen(
    raw_bytes: Vec<u8>,
    preopens: Vec<PathBuf>,
    init_func: String,
    keep_init_func: bool,
) -> anyhow::Result<Vec<u8>> {
    let mut config = Config::new();
    config.wasm_bulk_memory(true);
    let engine = Engine::new(&config)?;

    let mut wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new();
    configure_wasi(&mut wasi_ctx, &preopens)?;

    let mut store = Store::new(&engine, wasi_ctx.build_p1());
    let mut linker = Linker::new(&engine);
    p1::add_to_linker_async(&mut linker, |cx| cx)?;

    // Preload the weval stubs module.
    let stubs_module = Module::new(&engine, STUBS)?;
    let stubs_instance = linker.instantiate_async(&mut store, &stubs_module).await?;
    linker.instance(&mut store, "weval", stubs_instance)?;

    let mut wizer = wasmtime_wizer::Wizer::new();
    wizer.init_func(&init_func);

    if keep_init_func {
        wizer.keep_init_func(true);
    }

    wizer.func_rename("_start", "wizer.resume");

    Ok(wizer
        .run(&mut store, &raw_bytes, async |store, module| {
            linker.define_unknown_imports_as_traps(module)?;
            linker.instantiate_async(store, module).await
        })
        .await?)
}

pub(crate) fn weval(module_bytes: &[u8], opts: Opts) -> anyhow::Result<Vec<u8>> {
    // Open the cache and read-only cache, if any.
    let cache = cache::Cache::open(
        opts.cache.as_deref(),
        opts.cache_ro.as_deref(),
        opts.module_hash,
    )?;

    // Load module.
    if opts.verbose {
        eprintln!("Parsing the module...");
    }
    let mut frontend_opts = waffle::FrontendOptions::default();
    frontend_opts.debug = true;
    let module = waffle::Module::from_wasm_bytes(module_bytes, &frontend_opts)?;

    // Build module image.
    if opts.verbose {
        eprintln!("Building memory image...");
    }
    let mut im = image::build_image(&module, None)?;

    // Collect directives.
    let directives = directive::collect(&module, &mut im)?;
    log::debug!("Directives: {directives:?}");

    // Make sure IR output directory exists.
    if let Some(dir) = &opts.output_ir {
        std::fs::create_dir_all(dir)?;
    }

    // Partially evaluate.
    if opts.verbose {
        eprintln!("Specializing functions...");
    }
    let progress = if opts.verbose {
        Some(indicatif::ProgressBar::new(0))
    } else {
        None
    };
    let mut result = eval::partially_evaluate(
        module,
        &mut im,
        &directives[..],
        progress,
        opts.output_ir,
        &cache,
    )?;

    // Update memories in module.
    if opts.verbose {
        eprintln!("Updating memory image...");
    }
    image::update(&mut result.module, &im);

    log::debug!("Final module:\n{}", result.module.display());

    if opts.show_stats {
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

    if opts.verbose {
        eprintln!("Serializing back to binary form...");
    }
    let bytes = result.module.to_wasm_bytes()?;

    if opts.verbose {
        eprintln!("Performing post-filter pass to remove intrinsics...");
    }
    filter::filter(&bytes[..])
}
