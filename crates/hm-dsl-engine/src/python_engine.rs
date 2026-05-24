//! WASI-based Python DSL engine.
//!
//! Runs CPython compiled to `wasm32-wasi` via [`wasmtime`] to evaluate Python
//! pipeline definitions. The engine downloads `cpython.wasm` on first use
//! (cached at `~/.harmont/runtimes/`), embeds the `harmont` Python package and
//! vendored dependencies at compile time, and materialises them into a
//! temporary directory for each evaluation.

use std::path::Path;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tracing::debug;
use wasmtime::{Engine, Linker, Module, Store};
use wasmtime_wasi::p2::pipe::MemoryOutputPipe;
use wasmtime_wasi::p2::WasiCtxBuilder;
use wasmtime_wasi::preview1::{self, WasiP1Ctx};
use wasmtime_wasi::{DirPerms, FilePerms, I32Exit};

use crate::embedded_sources;
use crate::runtime_cache::{CPYTHON_WASI, RuntimeCache};
use crate::wasm_runtime;
use crate::{DslEngine, PipelineMeta};

// ---------------------------------------------------------------------------
// Python script constants (same as fallback.rs)
// ---------------------------------------------------------------------------

/// Python one-liner that discovers every `@hm.pipeline` registration in
/// `.harmont/*.py` and prints `[{slug, name}, ...]` JSON to stdout.
const LIST_PIPELINES_SCRIPT: &str = "\
import importlib.util, json, pathlib
import harmont as hm
for p in sorted(pathlib.Path('.harmont').glob('*.py')):
    spec = importlib.util.spec_from_file_location(f'_harmont_{p.stem}', p)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
envelope = json.loads(hm.dump_registry_json())
print(json.dumps([{'slug': p['slug'], 'name': p['name']} for p in envelope['pipelines']]))
";

/// Python script that renders a single pipeline (slug passed as `sys.argv[1]`)
/// to its JSON IR definition and prints it to stdout.
const RENDER_PIPELINE_SCRIPT: &str = "\
import importlib.util, json, pathlib, sys
import harmont as hm
slug = sys.argv[1]
for p in sorted(pathlib.Path('.harmont').glob('*.py')):
    spec = importlib.util.spec_from_file_location(f'_harmont_{p.stem}', p)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
envelope = json.loads(hm.dump_registry_json())
match = next((p for p in envelope['pipelines'] if p['slug'] == slug), None)
if match is None:
    avail = ', '.join(p['slug'] for p in envelope['pipelines']) or '(none)'
    print(f'error: pipeline {slug!r} not found\\n  -> available: {avail}', file=sys.stderr)
    sys.exit(2)
print(json.dumps(match['definition']))
";

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

/// A DSL engine that runs Python pipeline definitions inside CPython compiled
/// to WASI, hosted by [`wasmtime`].
#[derive(Debug)]
pub struct WasmPythonEngine {
    engine: Engine,
    module: Module,
}

impl WasmPythonEngine {
    /// Create a new engine, downloading and compiling `cpython.wasm` if
    /// necessary.
    ///
    /// The WASM module is compiled with Cranelift and cached on disk so that
    /// subsequent starts are fast.
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime cannot be downloaded/verified, or if
    /// the WASM module fails to compile.
    pub async fn new() -> Result<Self> {
        let engine = wasm_runtime::create_engine()?;
        let cache = RuntimeCache::default_path()?;
        let wasm_path = cache.ensure(&CPYTHON_WASI).await?;

        let engine_clone = engine.clone();
        let module = tokio::task::spawn_blocking(move || {
            Module::from_file(&engine_clone, &wasm_path)
        })
        .await?
        .context("compiling cpython.wasm")?;

        Ok(Self { engine, module })
    }

    /// Run a Python script inside the WASI sandbox and capture its stdout.
    ///
    /// The embedded `harmont` package and vendored dependencies are
    /// materialised into a temporary directory that is preopened for the
    /// guest. The `project_dir` is also preopened (read-only) so that the
    /// guest can read `.harmont/*.py` pipeline definitions.
    fn run_script(
        &self,
        project_dir: &Path,
        script: &str,
        extra_args: &[&str],
    ) -> Result<Vec<u8>> {
        // 1. Extract embedded harmont-py + vendor packages to a temp dir.
        let tmp = tempfile::tempdir().context("creating temp dir for Python packages")?;
        let harmont_pkg = tmp.path().join("harmont");
        embedded_sources::extract_to(&embedded_sources::HARMONT_PY, &harmont_pkg)?;
        embedded_sources::extract_to(&embedded_sources::VENDOR_PACKAGES, tmp.path())?;

        // 2. Build PYTHONPATH: temp dir (harmont + vendors) + project .harmont dir.
        let pythonpath = format!(
            "{}:{}",
            tmp.path().display(),
            project_dir.join(".harmont").display()
        );

        // 3. Build argv: ["python3", "-c", <script>, ...extra_args].
        let mut argv: Vec<String> =
            vec!["python3".into(), "-c".into(), script.into()];
        argv.extend(extra_args.iter().map(|s| s.to_string()));

        // 4. Set up WASI context with stdout/stderr capture and preopened dirs.
        //    Use a generous 64 MiB capacity for stdout — pipeline JSON can be
        //    large but not *that* large.
        let stdout_pipe = MemoryOutputPipe::new(64 * 1024 * 1024);
        let stderr_pipe = MemoryOutputPipe::new(1024 * 1024);

        let mut wasi_builder = WasiCtxBuilder::new();
        wasi_builder
            .stdout(stdout_pipe.clone())
            .stderr(stderr_pipe.clone())
            .args(&argv)
            .env("PYTHONPATH", &pythonpath)
            .env("LANG", "C.UTF-8")
            .allow_blocking_current_thread(true);

        // Preopen the temp dir (contains harmont pkg + vendors) as `/packages`.
        wasi_builder
            .preopened_dir(tmp.path(), "/packages", DirPerms::READ, FilePerms::READ)
            .context("preopening packages temp dir")?;

        // Preopen the project directory so CPython can read `.harmont/*.py`.
        wasi_builder
            .preopened_dir(project_dir, "/project", DirPerms::READ, FilePerms::READ)
            .context("preopening project dir")?;

        // Also preopen project dir as "." — many WASI programs expect a CWD
        // preopen, and the Python scripts use relative paths like
        // `pathlib.Path('.harmont')`.
        wasi_builder
            .preopened_dir(project_dir, ".", DirPerms::READ, FilePerms::READ)
            .context("preopening project dir as '.'")?;

        let wasi_ctx = wasi_builder.build_p1();

        // 5. Instantiate and run `_start`.
        let mut store: Store<WasiP1Ctx> = Store::new(&self.engine, wasi_ctx);
        let mut linker: Linker<WasiP1Ctx> = Linker::new(&self.engine);
        preview1::add_to_linker_sync(&mut linker, |ctx| ctx)?;

        let instance = linker
            .instantiate(&mut store, &self.module)
            .context("instantiating cpython.wasm")?;

        let start = instance
            .get_typed_func::<(), ()>(&mut store, "_start")
            .context("locating _start export")?;

        let run_result = start.call(&mut store, ());

        // 6. Collect output.
        let stdout_bytes = stdout_pipe.contents();
        let stderr_bytes = stderr_pipe.contents();

        match run_result {
            Ok(()) => Ok(stdout_bytes.to_vec()),
            Err(e) => {
                // CPython exits via `proc_exit(0)` which wasmtime surfaces as
                // an `I32Exit(0)` trap — treat exit code 0 as success.
                if let Some(exit) = e.downcast_ref::<I32Exit>() {
                    if exit.0 == 0 {
                        return Ok(stdout_bytes.to_vec());
                    }
                    let stderr_str = String::from_utf8_lossy(&stderr_bytes);
                    bail!(
                        "python exited with code {}: {}",
                        exit.0,
                        stderr_str.trim()
                    );
                }
                Err(e).context("running cpython.wasm")
            }
        }
    }
}

#[async_trait]
impl DslEngine for WasmPythonEngine {
    async fn list_pipelines(&self, project_dir: &Path) -> Result<Vec<PipelineMeta>> {
        let project_dir = project_dir.to_path_buf();
        // Safety: `self` is Send + Sync; we only borrow engine/module which are
        // safe to share across threads.
        let engine = self.engine.clone();
        let module = self.module.clone();

        let stdout = tokio::task::spawn_blocking(move || {
            let tmp_engine = WasmPythonEngine { engine, module };
            tmp_engine.run_script(&project_dir, LIST_PIPELINES_SCRIPT, &[])
        })
        .await?
        .context("listing pipelines via WASI Python")?;

        debug!(
            raw_len = stdout.len(),
            "list_pipelines stdout captured from WASI"
        );

        let metas: Vec<PipelineMeta> =
            serde_json::from_slice(&stdout).context("decoding pipeline metadata from WASI stdout")?;
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
            let tmp_engine = WasmPythonEngine { engine, module };
            tmp_engine.run_script(&project_dir, RENDER_PIPELINE_SCRIPT, &[&slug])
        })
        .await?
        .context("rendering pipeline via WASI Python")?;

        String::from_utf8(stdout).context("WASI Python stdout is not valid UTF-8")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that the engine can be constructed (requires cpython.wasm to be
    /// cached at `~/.harmont/runtimes/`).
    #[tokio::test]
    #[ignore = "requires cpython.wasm cached at ~/.harmont/runtimes/"]
    async fn creates_engine() {
        let engine = WasmPythonEngine::new().await.unwrap();
        drop(engine);
    }
}
