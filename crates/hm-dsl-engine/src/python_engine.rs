use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use hm_common::process::CapturedStreams as _;
use hm_core::app_ctx::AppCtx;
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
pub struct SubprocessPythonEngine<'app> {
    app: &'app AppCtx,
}

impl<'app> SubprocessPythonEngine<'app> {
    /// Create the engine bound to `app`, whose resolved `python3` it runs.
    #[must_use]
    pub const fn new(app: &'app AppCtx) -> Self {
        Self { app }
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

        let mut py = self.app.python().program(script);
        py.args(extra_args).current_dir(project_dir);
        py.pythonpath(tmp.path());

        debug!(?py, "running python3 subprocess");

        Ok(py.run().await?.stdout_string()?)
    }
}

#[async_trait]
impl DslEngine for SubprocessPythonEngine<'_> {
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
