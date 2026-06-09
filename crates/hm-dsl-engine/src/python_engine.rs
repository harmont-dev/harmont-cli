use std::path::Path;
use std::process::Stdio;

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use tracing::debug;

use crate::bundled_sources;
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

const RENDER_PIPELINE_SCRIPT: &str = "\
import sys, json, pathlib, importlib.util
try:
    import harmont as hm
except ImportError as e:
    print(f'error: {e}', file=sys.stderr)
    sys.exit(1)
slug = sys.argv[1]
for p in sorted(pathlib.Path('.hm').glob('*.py')):
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
        self.run_script(project_dir, RENDER_PIPELINE_SCRIPT, &[slug])
            .await
            .context("rendering pipeline via python3")
    }

    async fn registry_json(&self, project_dir: &Path) -> Result<String> {
        self.run_script(project_dir, REGISTRY_JSON_SCRIPT, &[])
            .await
            .context("dumping pipeline registry via python3")
    }
}
