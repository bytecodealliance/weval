#![allow(dead_code, reason = "")]

use component::WasiOpts;
use std::path::PathBuf;
use structopt::StructOpt;

use wasmparser::{Encoding, Parser, Payload};

mod cache;
mod component;
mod constant_offsets;
mod dce;
mod directive;
mod escape;
mod eval;
mod filter;
mod image;
mod intrinsics;
mod liveness;
mod module;
mod state;
mod stats;
mod value;

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

        /// Keep the Wizer initialization function exported after wizening.
        #[structopt(long = "keep-init-func")]
        keep_init_func: bool,

        /// Allow WASI Preview 2 imports while wizening components.
        #[structopt(long = "allow-wasip2")]
        allow_wasip2: bool,

        /// Allow WASI Preview 3 imports while wizening components.
        #[structopt(long = "allow-wasip3")]
        allow_wasip3: bool,

        /// Allow WASI HTTP imports while wizening components.
        #[structopt(long = "allow-wasi-http")]
        allow_wasi_http: bool,

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
            keep_init_func,
            allow_wasip2,
            allow_wasip3,
            allow_wasi_http,
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
            keep_init_func,
            cache,
            cache_ro,
            show_stats,
            output_ir,
            verbose,
            WasiOpts {
                p2: allow_wasip2,
                p3: allow_wasip3,
                http: allow_wasi_http,
            },
        ),
    }
}

fn wasm_encoding(wasm: &[u8]) -> anyhow::Result<Encoding> {
    for payload in Parser::new(0).parse_all(wasm) {
        if let Payload::Version { encoding, .. } = payload? {
            return Ok(encoding);
        }
    }
    anyhow::bail!("input is not a wasm module or component")
}

pub(crate) fn configure_wasi(
    wasi_ctx: &mut wasmtime_wasi::WasiCtxBuilder,
    preopens: &[PathBuf],
) -> anyhow::Result<()> {
    wasi_ctx.inherit_stdio();
    wasi_ctx.inherit_env();
    for preopen in preopens {
        wasi_ctx.preopened_dir(
            preopen,
            preopen.to_str().unwrap_or("."),
            wasmtime_wasi::DirPerms::all(),
            wasmtime_wasi::FilePerms::all(),
        )?;
    }
    Ok(())
}

fn wizen(
    wasm_bytes: Vec<u8>,
    preopens: Vec<PathBuf>,
    init_func: String,
    keep_init_func: bool,
    wasi: WasiOpts,
    encoding: Encoding,
) -> anyhow::Result<Vec<u8>> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        match encoding {
            Encoding::Module => {
                module::wizen(wasm_bytes, preopens, init_func, keep_init_func).await
            }
            Encoding::Component => {
                component::wizen(wasm_bytes, preopens, init_func, keep_init_func, wasi).await
            }
        }
    })
}

/// Weval a wasm.
fn weval(
    input_module: PathBuf,
    output_module: PathBuf,
    do_wizen: bool,
    preopens: Vec<PathBuf>,
    init_func: String,
    keep_init_func: bool,
    cache: Option<PathBuf>,
    cache_ro: Option<PathBuf>,
    show_stats: bool,
    output_ir: Option<PathBuf>,
    verbose: bool,
    wasi: WasiOpts,
) -> anyhow::Result<()> {
    if verbose {
        eprintln!("Reading raw module bytes...");
    }

    let raw_bytes = std::fs::read(&input_module)?;

    // Compute a hash of the original module so we can cache results
    // keyed on that hash (and weval request arg strings).
    let input_hash = cache::compute_hash(&raw_bytes[..]);
    let input_encoding = wasm_encoding(&raw_bytes)?;

    // Optionally, Wizen the module or component first.
    let wasm_bytes = if do_wizen {
        if verbose {
            let encoding = match input_encoding {
                Encoding::Module => "module",
                Encoding::Component => "component",
            };
            eprintln!("Wizening the {encoding} with its input...");
        }
        wizen(
            raw_bytes,
            preopens,
            init_func,
            keep_init_func,
            wasi,
            input_encoding,
        )?
    } else {
        raw_bytes
    };

    let bytes = match input_encoding {
        Encoding::Module => module::weval(
            &wasm_bytes,
            module::Opts {
                cache,
                cache_ro,
                module_hash: input_hash,
                show_stats,
                output_ir,
                verbose,
            },
        )?,
        Encoding::Component => component::weval(
            &wasm_bytes,
            component::Opts {
                cache,
                cache_ro,
                show_stats,
                output_ir,
                verbose,
            },
        )?,
    };

    if verbose {
        eprintln!("Writing output file...");
    }
    std::fs::write(&output_module, &bytes[..])?;

    if verbose {
        eprintln!("Done.");
    }
    Ok(())
}
