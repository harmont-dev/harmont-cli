use std::collections::BTreeMap;
use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, anyhow, bail};
use async_trait::async_trait;
use tracing::debug;

use crate::bundled_sources;
use crate::raw_envelope::{FinalEnvelope, RawEnvelope, process_raw_envelope_with_options};
use crate::{DslEngine, PipelineMeta};

const LIST_PIPELINES_SCRIPT: &str = "\
import sys, json, pathlib, importlib.util
try:
    import harmont as hm
except ImportError as e:
    print(f'error: {e}', file=sys.stderr)
    sys.exit(1)
for p in sorted(pathlib.Path('.hm').glob('*.py')):
    spec = importlib.util.spec_from_file_location(f'_harmont_{p.stem}', p)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
envelope = json.loads(hm.dump_registry_json())
print(json.dumps([{'slug': p['slug'], 'name': p['name']} for p in envelope['pipelines']]))
";

const REGISTRY_JSON_SCRIPT: &str = "\
import sys, pathlib, importlib.util
try:
    import harmont as hm
except ImportError as e:
    print(f'error: {e}', file=sys.stderr)
    sys.exit(1)
for p in sorted(pathlib.Path('.hm').glob('*.py')):
    spec = importlib.util.spec_from_file_location(f'_harmont_{p.stem}', p)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
sys.stdout.write(hm.dump_registry_json())
";

#[derive(Debug)]
pub struct SubprocessPythonEngine {
    python_bin: std::path::PathBuf,
}

impl SubprocessPythonEngine {
    /// Create engine, verifying `python3` is available on PATH.
    ///
    /// # Errors
    ///
    /// Returns an error if `python3` is not found on `PATH`.
    pub fn new() -> Result<Self> {
        let python_bin =
            which::which("python3").context("python3 not found on PATH — install Python 3.11+")?;
        Ok(Self { python_bin })
    }

    async fn run_script(
        &self,
        project_dir: &Path,
        script: &str,
        extra_args: &[&str],
    ) -> Result<String> {
        let tmp = tempfile::tempdir().context("creating temp dir for harmont-py")?;
        let harmont_pkg = tmp.path().join("harmont");
        bundled_sources::extract_to(&bundled_sources::HARMONT_PY, &harmont_pkg)?;

        let mut cmd = tokio::process::Command::new(&self.python_bin);
        cmd.arg("-c")
            .arg(script)
            .args(extra_args)
            .current_dir(project_dir)
            .env("PYTHONPATH", tmp.path())
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        debug!(?cmd, "running python3 subprocess");

        let output = cmd.output().await.context("spawning python3")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let code = output.status.code().unwrap_or(-1);
            bail!("python3 exited with code {code}:\n{stderr}");
        }

        String::from_utf8(output.stdout).context("python3 stdout is not valid UTF-8")
    }

    /// Run the Python discovery script, deserialize the raw step-chain
    /// envelope, and lower every pipeline into the v0 IR in Rust.
    ///
    /// Cache keys are resolved here (not in Python) using the same inputs the
    /// legacy Python resolver used: `pipeline_org` from `HM_PIPELINE_ORG`
    /// (falling back to `"default"`), the current unix time, `project_dir` as
    /// the `on_change` base path, and the process environment.
    async fn run_and_process_envelope(&self, project_dir: &Path) -> Result<FinalEnvelope> {
        let raw_json = self
            .run_script(project_dir, REGISTRY_JSON_SCRIPT, &[])
            .await?;
        let raw: RawEnvelope =
            serde_json::from_str(&raw_json).context("parsing raw envelope from Python")?;

        let env: BTreeMap<String, String> = std::env::vars().collect();
        let org = env
            .get("HM_PIPELINE_ORG")
            .cloned()
            .unwrap_or_else(|| "default".to_owned());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .context("system clock is before the unix epoch")?
            .as_secs();

        process_raw_envelope_with_options(raw, &org, now, project_dir, &env)
    }
}

#[async_trait]
impl DslEngine for SubprocessPythonEngine {
    async fn list_pipelines(&self, project_dir: &Path) -> Result<Vec<PipelineMeta>> {
        let stdout = self
            .run_script(project_dir, LIST_PIPELINES_SCRIPT, &[])
            .await
            .context("listing pipelines via python3")?;

        debug!(raw_len = stdout.len(), "list_pipelines stdout");

        serde_json::from_str(&stdout).context("decoding pipeline metadata from python3 stdout")
    }

    async fn render_pipeline_json(&self, project_dir: &Path, slug: &str) -> Result<String> {
        let envelope = self.run_and_process_envelope(project_dir).await?;
        let entry = envelope
            .pipelines
            .iter()
            .find(|p| p.slug == slug)
            .ok_or_else(|| {
                let avail: String = envelope
                    .pipelines
                    .iter()
                    .map(|p| p.slug.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                let avail = if avail.is_empty() { "(none)" } else { &avail };
                anyhow!("pipeline {slug:?} not found\n  -> available: {avail}")
            })?;
        serde_json::to_string(&entry.definition)
            .with_context(|| format!("serializing definition for pipeline {slug:?}"))
    }

    async fn registry_json(&self, project_dir: &Path) -> Result<String> {
        let envelope = self.run_and_process_envelope(project_dir).await?;
        serde_json::to_string(&envelope).context("serializing lowered discovery envelope")
    }
}

/// Instanciates a python engine.
/// Shorthand for [`SubprocessPythonEngine`].
///
/// # Errors
///
/// Returns an error if `python3` is not found on `PATH`.
#[inline]
pub fn engine() -> Result<SubprocessPythonEngine> {
    SubprocessPythonEngine::new()
}
