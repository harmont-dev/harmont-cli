//! Render the pipeline registered under `<slug>` to v0 IR JSON.
//!
//! `python3` walks every `.harmont/*.py`, executes each (so every
//! `@hm.pipeline` decorator self-registers), then emits the
//! `schema_version="1"` envelope JSON. The host filters by slug and
//! returns the v0 IR `definition` as JSON.
//!
//! Mirrors `Harmont.Executor.Render.renderPipeline` on the api side.

use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::process::Command;

use crate::error::HmError;

/// Metadata for one `@hm.pipeline` registration. Used by the
/// "no slug → list available" branch in the caller.
#[derive(Debug, Deserialize)]
pub(super) struct PipelineMeta {
    pub(super) slug: String,
    // Part of the JSON envelope the python harness emits; kept so
    // serde succeeds against the registry shape. Not read today —
    // future "list pipelines" UI will render it.
    #[allow(dead_code)]
    pub(super) name: String,
}

/// Walk every `.harmont/*.py` in `repo_root`, execute each so the
/// `@hm.pipeline` decorators self-register, then return a flat list of
/// `(slug, name)` for the human-facing "no slug" error path.
///
/// # Errors
///
/// Returns an error if `python3` cannot be spawned, the Python harness
/// exits non-zero (DSL bug, missing import, duplicate slug), or its
/// stdout is not valid JSON. Errors carry the offending process's
/// stderr verbatim where available.
pub(super) async fn list_pipelines(repo_root: &Path) -> Result<Vec<PipelineMeta>> {
    let script = "\
import importlib.util, json, pathlib
import harmont as hm
for p in sorted(pathlib.Path('.harmont').glob('*.py')):
    spec = importlib.util.spec_from_file_location(f'_harmont_{p.stem}', p)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
envelope = json.loads(hm.dump_registry_json())
print(json.dumps([{'slug': p['slug'], 'name': p['name']} for p in envelope['pipelines']]))
";

    let py = Command::new("python3")
        .arg("-c")
        .arg(script)
        .env("PYTHONPATH", repo_root.join(".harmont"))
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn python3")?;
    let py_out = py.wait_with_output().await.context("wait python3")?;
    if !py_out.status.success() {
        return Err(HmError::PipelineRender(format!(
            "python3 exited {}: {}",
            py_out.status,
            String::from_utf8_lossy(&py_out.stderr)
        ))
        .into());
    }
    let out: Vec<PipelineMeta> =
        serde_json::from_slice(&py_out.stdout).context("decode pipeline metadata")?;
    Ok(out)
}

/// Render the pipeline registered under `slug` to v0 IR JSON.
///
/// # Errors
///
/// Returns an error if `python3` cannot be spawned, the python script
/// exits non-zero (DSL bug, missing import, slug not found), or its
/// stdout is not valid UTF-8. Errors carry the offending process's
/// stderr verbatim where available.
pub(super) async fn render_pipeline_json(
    repo_root: &Path,
    slug: &str,
) -> Result<Vec<u8>> {
    let render_script = "\
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

    let py = Command::new("python3")
        .arg("-c")
        .arg(render_script)
        .arg(slug)
        .env("PYTHONPATH", repo_root.join(".harmont"))
        .current_dir(repo_root)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn python3")?;
    let py_out = py.wait_with_output().await.context("wait python3")?;
    if !py_out.status.success() {
        return Err(HmError::PipelineRender(format!(
            "python3 exited {}: {}",
            py_out.status,
            String::from_utf8_lossy(&py_out.stderr)
        ))
        .into());
    }
    Ok(py_out.stdout)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[tokio::test]
    #[cfg_attr(
        not(feature = "py-env"),
        ignore = "requires the `harmont` Python package on PATH; enable via --features py-env"
    )]
    async fn renders_a_minimal_pipeline_to_json() {
        let dir = tempdir().expect("tempdir");
        let harmont = dir.path().join(".harmont");
        fs::create_dir_all(&harmont).expect("mkdir .harmont");
        fs::write(
            harmont.join("demo.py"),
            "import harmont as hm\n\n@hm.pipeline('demo')\ndef demo() -> hm.Step:\n    return hm.scratch().run('echo hi', label='hello')\n",
        )
        .expect("write pipeline file");

        let json = render_pipeline_json(dir.path(), "demo")
            .await
            .expect("render ok");
        let s = std::str::from_utf8(&json).expect("utf-8");
        assert!(s.contains("\"version\": \"0\""), "json was: {s}");
        assert!(s.contains("\"hello\""), "json was: {s}");
    }
}
