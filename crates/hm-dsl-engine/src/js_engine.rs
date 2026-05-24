//! WASI-based JavaScript DSL engine (QuickJS).
//!
//! Runs QuickJS compiled to `wasm32-wasi` via [`wasmtime`] to evaluate
//! TypeScript pipeline definitions. TypeScript sources are first preprocessed
//! by [`crate::ts_preprocess`] (type-stripping + import rewriting) and then
//! concatenated with the build-time `harmont-ts` IIFE bundle before being
//! handed to QuickJS for evaluation.

use std::path::Path;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tracing::debug;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::p2::WasiCtxBuilder;
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::I32Exit;

use crate::ts_preprocess::preprocess_ts;
use crate::wasm_runtime;
use crate::{DslEngine, PipelineMeta};

// ---------------------------------------------------------------------------
// Embedded assets
// ---------------------------------------------------------------------------

/// QuickJS compiled to wasm32-wasi, bundled at build time.
static QUICKJS_WASM: &[u8] = include_bytes!("../embedded/quickjs.wasm");

/// The `harmont-ts` IIFE bundle produced by esbuild during `build.rs`.
/// Defines `globalThis.harmont` with `renderEnvelope`, `pipeline`, `sh`, etc.
const HARMONT_BUNDLE: &str = include_str!(concat!(env!("OUT_DIR"), "/harmont-bundle.js"));

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// A DSL engine that runs TypeScript pipeline definitions inside QuickJS
/// compiled to WASI, hosted by [`wasmtime`].
#[derive(Debug)]
pub struct WasmJsEngine {
    engine: Engine,
    module: Module,
}

impl WasmJsEngine {
    /// Create a new engine, compiling the embedded `quickjs.wasm` module.
    ///
    /// The WASM module is compiled with Cranelift. Wasmtime's on-disk cache
    /// ensures subsequent starts are fast.
    ///
    /// # Errors
    ///
    /// Returns an error if the WASM module fails to compile.
    pub fn new() -> Result<Self> {
        let engine = wasm_runtime::create_engine()?;

        let module = Module::new(&engine, QUICKJS_WASM)
            .context("compiling quickjs.wasm")?;

        Ok(Self { engine, module })
    }

    /// Run a JavaScript script inside QuickJS-WASI and capture its stdout.
    ///
    /// QuickJS is invoked as `qjs --std -e <script>`. The `--std` flag loads
    /// the `std` and `os` QuickJS modules, making them available as globals.
    /// Output is captured via `print()` (QuickJS built-in that writes to
    /// stdout).
    fn eval_js(&self, script: &str) -> Result<Vec<u8>> {
        let stdout_pipe = MemoryOutputPipe::new(64 * 1024 * 1024);
        let stderr_pipe = MemoryOutputPipe::new(1024 * 1024);

        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder
            .stdout(stdout_pipe.clone())
            .stderr(stderr_pipe.clone())
            .args(&["qjs", "--std", "-e", script])
            .allow_blocking_current_thread(true);

        let wasi_ctx = wasi_builder.build_p1();

        let mut store: Store<WasiP1Ctx> = Store::new(&self.engine, wasi_ctx);
        let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);
        preview1::add_to_linker_sync(&mut linker, |ctx| ctx)?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .context("instantiating quickjs.wasm")?;

        let start = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .context("locating _start export")?;

        let run_result = start.call(&mut store, ());

        let stdout_bytes = stdout_pipe.contents();
        let stderr_bytes = stderr_pipe.contents();

        match run_result {
            Ok(()) => Ok(stdout_bytes.to_vec()),
            Err(e) => {
                // QuickJS exits via `proc_exit(0)` — wasmtime surfaces this as
                // `I32Exit(0)`. Treat exit code 0 as success.
                if let Some(exit) = e.downcast_ref::<I32Exit>() {
                    if exit.0 == 0 {
                        return Ok(stdout_bytes.to_vec());
                    }
                    let stderr_str = String::from_utf8_lossy(&stderr_bytes);
                    bail!(
                        "quickjs exited with code {}: {}",
                        exit.0,
                        stderr_str.trim()
                    );
                }
                Err(e).context("running quickjs.wasm")
            }
        }
    }

    /// Evaluate TypeScript pipeline definitions from a project directory.
    ///
    /// 1. Loads the `harmont-ts` IIFE bundle (sets up `globalThis.harmont`).
    /// 2. Reads and preprocesses each `.harmont/*.ts` file (strip types,
    ///    rewrite imports).
    /// 3. Wraps each preprocessed file in an IIFE that captures
    ///    `export default` via a `module.exports` shim.
    /// 4. Builds a footer that collects all default exports, calls
    ///    `harmont.renderEnvelope()`, optionally filters by slug, and prints
    ///    the result.
    /// 5. Runs the concatenated script through QuickJS.
    fn eval_ts_pipeline(
        &self,
        project_dir: &Path,
        slug: Option<&str>,
    ) -> Result<String> {
        let harmont_dir = project_dir.join(".harmont");
        if !harmont_dir.is_dir() {
            bail!(
                "no .harmont/ directory found in {}",
                project_dir.display()
            );
        }

        // Collect .ts files, sorted for determinism.
        let mut ts_files: Vec<_> = std::fs::read_dir(&harmont_dir)
            .with_context(|| format!("reading {}", harmont_dir.display()))?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension().is_some_and(|ext| ext == "ts") {
                    Some(path)
                } else {
                    None
                }
            })
            .collect();
        ts_files.sort();

        if ts_files.is_empty() {
            bail!(
                "no .ts files found in {}",
                harmont_dir.display()
            );
        }

        // Start building the full evaluation script.
        let mut script = String::with_capacity(HARMONT_BUNDLE.len() + 4096);

        // 1. harmont-ts bundle (defines globalThis.harmont).
        script.push_str(HARMONT_BUNDLE);
        script.push('\n');

        // 2. Process each .ts file.
        let mut file_vars = Vec::with_capacity(ts_files.len());

        for (i, ts_path) in ts_files.iter().enumerate() {
            let source = std::fs::read_to_string(ts_path)
                .with_context(|| format!("reading {}", ts_path.display()))?;

            let js = preprocess_ts(&source)
                .with_context(|| format!("preprocessing {}", ts_path.display()))?;

            // Rewrite `export default` to `module.exports.default =` so it
            // works inside a non-module IIFE context.
            let js = rewrite_export_default(&js);

            let var_name = format!("__file_{i}");
            file_vars.push(var_name.clone());

            // Wrap in IIFE with exports/module shim to capture default export.
            script.push_str(&format!(
                "var {var_name} = (function() {{\n\
                 var exports = {{}};\n\
                 var module = {{ exports: exports }};\n\
                 {js}\n\
                 return exports[\"default\"] || module.exports[\"default\"] || module.exports;\n\
                 }})();\n"
            ));
        }

        // 3. Footer: collect default exports, render envelope, print.
        script.push_str("var __defs = [];\n");
        for var_name in &file_vars {
            script.push_str(&format!(
                "if ({var_name}) __defs = __defs.concat(\
                 Array.isArray({var_name}) ? {var_name} : [{var_name}]);\n"
            ));
        }

        script.push_str("var __envelope = harmont.renderEnvelope(__defs);\n");
        script.push_str("var __parsed = JSON.parse(__envelope);\n");

        match slug {
            Some(s) => {
                // Filter to a single pipeline and output its definition.
                script.push_str(&format!(
                    "var __match = null;\n\
                     for (var __i = 0; __i < __parsed.pipelines.length; __i++) {{\n\
                       if (__parsed.pipelines[__i].slug === '{s}') {{\n\
                         __match = __parsed.pipelines[__i];\n\
                         break;\n\
                       }}\n\
                     }}\n\
                     if (__match === null) {{\n\
                       var __avail = [];\n\
                       for (var __j = 0; __j < __parsed.pipelines.length; __j++) {{\n\
                         __avail.push(__parsed.pipelines[__j].slug);\n\
                       }}\n\
                       std.err.printf('error: pipeline \\'{s}\\' not found\\n  -> available: ' + (__avail.join(', ') || '(none)') + '\\n');\n\
                       std.exit(2);\n\
                     }}\n\
                     print(JSON.stringify(__match.definition));\n"
                ));
            }
            None => {
                // List mode: output [{slug, name}, ...].
                script.push_str(
                    "var __metas = [];\n\
                     for (var __i = 0; __i < __parsed.pipelines.length; __i++) {\n\
                       __metas.push({ slug: __parsed.pipelines[__i].slug, name: __parsed.pipelines[__i].name });\n\
                     }\n\
                     print(JSON.stringify(__metas));\n"
                );
            }
        }

        debug!(script_len = script.len(), "evaluating TS pipeline via QuickJS");

        let stdout = self
            .eval_js(&script)
            .context("evaluating TypeScript pipeline via QuickJS-WASI")?;

        String::from_utf8(stdout).context("QuickJS stdout is not valid UTF-8")
    }
}

/// Rewrite `export default <expr>` to `module.exports.default = <expr>`.
///
/// After oxc type-stripping with `with_module(true)`, `export default [...]`
/// remains as ESM syntax. QuickJS in script mode (`-e`) does not support ESM
/// exports, so we rewrite them to CommonJS-style assignments that the IIFE
/// wrapper can capture.
fn rewrite_export_default(js: &str) -> String {
    let mut output = String::with_capacity(js.len());
    for line in js.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("export default ") {
            // Replace `export default X` with `module.exports.default = X`
            let indent = &line[..line.len() - trimmed.len()];
            let rest = &trimmed["export default ".len()..];
            output.push_str(indent);
            output.push_str("module.exports.default = ");
            output.push_str(rest);
            output.push('\n');
        } else if trimmed == "export default" {
            // `export default` on its own line (expression on next line)
            let indent = &line[..line.len() - trimmed.len()];
            output.push_str(indent);
            output.push_str("module.exports.default =");
            output.push('\n');
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}

// ---------------------------------------------------------------------------
// DslEngine impl
// ---------------------------------------------------------------------------

#[async_trait]
impl DslEngine for WasmJsEngine {
    async fn list_pipelines(&self, project_dir: &Path) -> Result<Vec<PipelineMeta>> {
        let project_dir = project_dir.to_path_buf();
        let engine = self.engine.clone();
        let module = self.module.clone();

        let stdout = tokio::task::spawn_blocking(move || {
            let tmp_engine = WasmJsEngine { engine, module };
            tmp_engine.eval_ts_pipeline(&project_dir, None)
        })
        .await?
        .context("listing pipelines via QuickJS-WASI")?;

        debug!(
            raw_len = stdout.len(),
            "list_pipelines stdout captured from QuickJS-WASI"
        );

        let metas: Vec<PipelineMeta> = serde_json::from_str(&stdout)
            .context("decoding pipeline metadata from QuickJS stdout")?;
        Ok(metas)
    }

    async fn render_pipeline_json(
        &self,
        project_dir: &Path,
        slug: &str,
    ) -> Result<String> {
        let project_dir = project_dir.to_path_buf();
        let slug = slug.to_string();
        let engine = self.engine.clone();
        let module = self.module.clone();

        let stdout = tokio::task::spawn_blocking(move || {
            let tmp_engine = WasmJsEngine { engine, module };
            tmp_engine.eval_ts_pipeline(&project_dir, Some(&slug))
        })
        .await?
        .context("rendering pipeline via QuickJS-WASI")?;

        Ok(stdout)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rewrite_export_default_array() {
        let input = "const x = 1;\nexport default [\n  { slug: 'ci' }\n];\n";
        let output = rewrite_export_default(input);
        assert!(
            output.contains("module.exports.default = ["),
            "output: {output}"
        );
        assert!(!output.contains("export default"), "output: {output}");
    }

    #[test]
    fn rewrite_export_default_value() {
        let input = "export default myVar;\n";
        let output = rewrite_export_default(input);
        assert_eq!(output, "module.exports.default = myVar;\n");
    }

    #[test]
    fn rewrite_preserves_non_export_lines() {
        let input = "const x = 1;\nconst y = 2;\n";
        let output = rewrite_export_default(input);
        assert_eq!(output, input);
    }

    #[test]
    fn engine_creates_successfully() {
        let engine = WasmJsEngine::new().expect("engine should be created");
        drop(engine);
    }

    #[test]
    fn eval_js_hello_world() {
        let engine = WasmJsEngine::new().expect("engine should be created");
        let stdout = engine
            .eval_js("print('hello from quickjs');")
            .expect("eval_js should succeed");
        let output = String::from_utf8(stdout).unwrap();
        assert!(
            output.contains("hello from quickjs"),
            "output: {output}"
        );
    }

    #[test]
    fn eval_js_json_stringify() {
        let engine = WasmJsEngine::new().expect("engine should be created");
        let stdout = engine
            .eval_js("print(JSON.stringify({slug: 'ci', name: 'CI'}))")
            .expect("eval_js should succeed");
        let output = String::from_utf8(stdout).unwrap();
        assert!(output.contains(r#""slug":"ci""#), "output: {output}");
    }

    #[test]
    fn eval_js_std_module_available() {
        let engine = WasmJsEngine::new().expect("engine should be created");
        // Verify --std makes std/os globals available.
        let stdout = engine
            .eval_js("print(typeof std)")
            .expect("eval_js should succeed");
        let output = String::from_utf8(stdout).unwrap();
        assert!(
            output.trim() == "object",
            "std should be an object, got: {output}"
        );
    }

}
