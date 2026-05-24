//! Subprocess-based DSL engines that shell out to system `python3` or `node`.
//!
//! These serve as always-available fallbacks when the embedded (WASI) engines
//! are compiled out or otherwise unavailable.

use std::path::{Path, PathBuf};
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tokio::process::Command;

use crate::{DslEngine, PipelineMeta};

// ---------------------------------------------------------------------------
// Shared Python script constants
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

// ===========================================================================
// SystemPythonEngine
// ===========================================================================

/// Subprocess engine that delegates to the system `python3`.
///
/// This mirrors the behaviour in `crates/hm/src/commands/run/render.rs` and
/// is the fallback when the `embedded-python` feature is not compiled in.
#[derive(Debug)]
pub struct SystemPythonEngine {
    cidsl_py: PathBuf,
}

impl SystemPythonEngine {
    /// Discover the `cidsl/py` package path.
    ///
    /// Honors `HARMONT_CIDSL_PY` if set; otherwise walks up from the current
    /// executable looking for a sibling `cidsl/py` directory.
    ///
    /// # Errors
    ///
    /// Returns an error only if `std::env::current_exe` fails.
    pub fn discover() -> Result<Self> {
        let cidsl_py = if let Some(p) = std::env::var_os("HARMONT_CIDSL_PY") {
            PathBuf::from(p)
        } else {
            let exe = std::env::current_exe().context("locating cli executable")?;
            exe.ancestors()
                .find_map(|d| {
                    let candidate = d.join("cidsl/py");
                    candidate.exists().then_some(candidate)
                })
                .unwrap_or_else(|| PathBuf::from("cidsl/py"))
        };
        Ok(Self { cidsl_py })
    }

    /// Build the `PYTHONPATH` value for a given project root.
    fn pythonpath(&self, project_dir: &Path) -> String {
        format!(
            "{}:{}",
            self.cidsl_py.display(),
            project_dir.join(".harmont").display()
        )
    }

    /// Spawn `python3 -c <script>` with sanitised environment.
    fn base_command(&self, project_dir: &Path, script: &str) -> Command {
        let mut cmd = Command::new("python3");
        cmd.arg("-c")
            .arg(script)
            .env_clear()
            .env("PYTHONPATH", self.pythonpath(project_dir))
            .env("PATH", "/usr/bin:/usr/local/bin:/bin")
            .env("LANG", "C.UTF-8")
            .current_dir(project_dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }
}

#[async_trait]
impl DslEngine for SystemPythonEngine {
    async fn list_pipelines(&self, project_dir: &Path) -> Result<Vec<PipelineMeta>> {
        let py = self
            .base_command(project_dir, LIST_PIPELINES_SCRIPT)
            .spawn()
            .context("spawn python3")?;

        let output = py.wait_with_output().await.context("wait python3")?;

        if !output.status.success() {
            bail!(
                "python3 exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        let metas: Vec<PipelineMeta> =
            serde_json::from_slice(&output.stdout).context("decode pipeline metadata")?;
        Ok(metas)
    }

    async fn render_pipeline_json(
        &self,
        project_dir: &Path,
        slug: &str,
    ) -> Result<String> {
        let py = self
            .base_command(project_dir, RENDER_PIPELINE_SCRIPT)
            .arg(slug)
            .spawn()
            .context("spawn python3")?;

        let output = py.wait_with_output().await.context("wait python3")?;

        if !output.status.success() {
            bail!(
                "python3 exited {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr)
            );
        }

        String::from_utf8(output.stdout).context("python3 stdout is not valid UTF-8")
    }
}

// ===========================================================================
// SystemNodeEngine
// ===========================================================================

/// Stub subprocess engine for TypeScript pipelines via system `node`/`deno`.
///
/// Discovery verifies that a JS runtime is available on `PATH`, but actual
/// operations are not yet implemented — callers should prefer the embedded
/// TypeScript engine when available.
#[derive(Debug)]
pub struct SystemNodeEngine;

impl SystemNodeEngine {
    /// Check that `node` or `deno` is reachable on `PATH`.
    ///
    /// # Errors
    ///
    /// Returns an error if neither runtime can be found.
    pub fn discover() -> Result<Self> {
        let found = std::process::Command::new("node")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok()
            || std::process::Command::new("deno")
                .arg("--version")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .is_ok();

        if found {
            Ok(Self)
        } else {
            bail!("neither node nor deno found on PATH")
        }
    }
}

#[async_trait]
impl DslEngine for SystemNodeEngine {
    async fn list_pipelines(&self, _project_dir: &Path) -> Result<Vec<PipelineMeta>> {
        bail!("SystemNodeEngine: TypeScript pipeline listing via system Node is not yet implemented")
    }

    async fn render_pipeline_json(
        &self,
        _project_dir: &Path,
        _slug: &str,
    ) -> Result<String> {
        bail!(
            "SystemNodeEngine: TypeScript pipeline rendering via system Node is not yet implemented"
        )
    }
}
