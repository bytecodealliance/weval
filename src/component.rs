use std::path::PathBuf;

use anyhow::Context;
use wasmparser::{Encoding, Parser, Payload};
use wasmtime::component::Linker;
use wasmtime::{Config, Engine, Store};
use wasmtime_wasi::{p2, p3, ResourceTable, WasiCtx, WasiCtxView, WasiView};
use wasmtime_wasi_http::{p2 as http_p2, p3 as http_p3, WasiHttpCtx};

use crate::{cache, configure_wasi};

pub(crate) struct Opts {
    pub(crate) cache: Option<PathBuf>,
    pub(crate) cache_ro: Option<PathBuf>,
    pub(crate) show_stats: bool,
    pub(crate) output_ir: Option<PathBuf>,
    pub(crate) verbose: bool,
}

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct WasiOpts {
    pub(crate) p2: bool,
    pub(crate) p3: bool,
    pub(crate) http: bool,
}

struct ComponentWasiCtx {
    wasi: WasiCtx,
    http: WasiHttpCtx,
    table: ResourceTable,
}

impl WasiView for ComponentWasiCtx {
    fn ctx(&mut self) -> WasiCtxView<'_> {
        WasiCtxView {
            ctx: &mut self.wasi,
            table: &mut self.table,
        }
    }
}

impl http_p2::WasiHttpView for ComponentWasiCtx {
    fn http(&mut self) -> http_p2::WasiHttpCtxView<'_> {
        http_p2::WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: Default::default(),
        }
    }
}

impl http_p3::WasiHttpView for ComponentWasiCtx {
    fn http(&mut self) -> http_p3::WasiHttpCtxView<'_> {
        http_p3::WasiHttpCtxView {
            ctx: &mut self.http,
            table: &mut self.table,
            hooks: Default::default(),
        }
    }
}

pub(crate) async fn wizen(
    component_bytes: Vec<u8>,
    preopens: Vec<PathBuf>,
    init_func: String,
    keep_init_func: bool,
    wasi: WasiOpts,
) -> anyhow::Result<Vec<u8>> {
    let mut config = Config::new();
    config.wasm_bulk_memory(true);
    config.wasm_component_model(true);
    config.wasm_component_model_async(true);

    let engine = Engine::new(&config)?;

    let mut wasi_ctx = wasmtime_wasi::WasiCtxBuilder::new();
    configure_wasi(&mut wasi_ctx, &preopens)?;

    let ctx = ComponentWasiCtx {
        wasi: wasi_ctx.build(),
        http: WasiHttpCtx::new(),
        table: ResourceTable::new(),
    };

    let mut store = Store::new(&engine, ctx);
    let mut linker = Linker::new(&engine);

    if wasi.p2 {
        p2::add_to_linker_async(&mut linker)?;
    }
    if wasi.p3 {
        p3::add_to_linker(&mut linker)?;
    }

    if wasi.http {
        if !wasi.p2 && !wasi.p3 {
            anyhow::bail!("--allow-wasi-http requires --allow-wasip2 and/or --allow-wasip3");
        }
        if wasi.p2 {
            http_p2::add_only_http_to_linker_async(&mut linker)?;
        }
        if wasi.p3 {
            http_p3::add_to_linker(&mut linker)?;
        }
    }

    let mut wizer = wasmtime_wizer::Wizer::new();
    wizer.init_func(&init_func);
    if keep_init_func {
        wizer.keep_init_func(true);
    }

    Ok(wizer
        .run_component(&mut store, &component_bytes, async |store, component| {
            linker.instantiate_async(store, component).await
        })
        .await?)
}

pub(crate) fn weval(component_bytes: &[u8], opts: Opts) -> anyhow::Result<Vec<u8>> {
    // This follows the same high-level structure as Wizer's component rewrite:
    // See: https://github.com/bytecodealliance/wasmtime/blob/v46.0.1/crates/wizer/src/component/rewrite.rs
    let mut encoder = wasm_encoder::Component::new();
    let mut payloads = Parser::new(0).parse_all(component_bytes);
    let mut core_module_index = 0u32;

    while let Some(payload) = payloads.next() {
        match payload? {
            Payload::Version {
                encoding: Encoding::Component,
                ..
            } => {}
            Payload::Version {
                encoding: Encoding::Module,
                ..
            } => anyhow::bail!("expected a component, found a core module"),
            Payload::ModuleSection {
                unchecked_range, ..
            } => {
                let module_bytes = component_bytes
                    .get(unchecked_range.clone())
                    .context("component core module range is out of bounds")?;

                let rewritten_module = if needs_weval(module_bytes)? {
                    if opts.verbose {
                        eprintln!("Processing component core module {core_module_index}...");
                    }

                    let output_ir = opts
                        .output_ir
                        .as_ref()
                        .map(|dir| dir.join(format!("core-module-{core_module_index}")));

                    crate::module::weval(
                        module_bytes,
                        crate::module::Opts {
                            cache: opts.cache.clone(),
                            cache_ro: opts.cache_ro.clone(),
                            module_hash: cache::compute_hash(module_bytes),
                            show_stats: opts.show_stats,
                            output_ir,
                            verbose: opts.verbose,
                        },
                    )?
                } else {
                    module_bytes.to_vec()
                };

                encoder.section(&wasm_encoder::RawSection {
                    id: wasm_encoder::ComponentSectionId::CoreModule as u8,
                    data: &rewritten_module,
                });

                core_module_index += 1;
                skip_nested(&mut payloads)?;
            }
            Payload::ComponentSection {
                unchecked_range, ..
            } => {
                let nested_component = component_bytes
                    .get(unchecked_range.clone())
                    .context("nested component range is out of bounds")?;

                encoder.section(&wasm_encoder::RawSection {
                    id: wasm_encoder::ComponentSectionId::Component as u8,
                    data: nested_component,
                });

                skip_nested(&mut payloads)?;
            }
            Payload::End(_) => {}

            payload => {
                if let Some((id, range)) = payload.as_section() {
                    encoder.section(&wasm_encoder::RawSection {
                        id,
                        data: component_bytes
                            .get(range)
                            .context("component section range is out of bounds")?,
                    });
                }
            }
        }
    }

    let bytes = encoder.finish();

    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&bytes)
        .map(|_| ())
        .context("rewritten component failed validation")?;

    Ok(bytes)
}

fn needs_weval(module: &[u8]) -> anyhow::Result<bool> {
    for payload in Parser::new(0).parse_all(module) {
        match payload? {
            Payload::ImportSection(imports) => {
                for import in imports.into_imports() {
                    if import?.module == "weval" {
                        return Ok(true);
                    }
                }
            }
            Payload::ExportSection(exports) => {
                for export in exports {
                    if export?.name.starts_with("weval.") {
                        return Ok(true);
                    }
                }
            }
            Payload::End(_) => break,
            _ => {}
        }
    }
    Ok(false)
}

fn skip_nested<'a>(
    payloads: &mut impl Iterator<Item = wasmparser::Result<Payload<'a>>>,
) -> anyhow::Result<()> {
    let mut depth = 1usize;
    for payload in payloads {
        match payload? {
            Payload::ModuleSection { .. } | Payload::ComponentSection { .. } => {
                depth += 1;
            }
            Payload::End(_) => {
                depth -= 1;
                if depth == 0 {
                    return Ok(());
                }
            }
            _ => {}
        }
    }
    anyhow::bail!("nested wasm section did not terminate")
}
