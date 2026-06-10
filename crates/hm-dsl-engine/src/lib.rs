use std::collections::BTreeMap;
use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub mod detect;
pub mod python_engine;
pub mod ts_engine;

mod bundled_sources;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DslLanguage {
    Python,
    TypeScript,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineMeta {
    pub slug: String,
    pub name: String,
}

/// Runtime values available while evaluating a deferred DSL target.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DynamicContext {
    /// Explicit environment supplied for the build.
    pub env: BTreeMap<String, String>,
}

#[async_trait]
pub trait DslEngine: Send + Sync + std::fmt::Debug {
    async fn list_pipelines(&self, project_dir: &Path) -> anyhow::Result<Vec<PipelineMeta>>;
    async fn render_pipeline_json(&self, project_dir: &Path, slug: &str) -> anyhow::Result<String>;
    /// Evaluate one registered dynamic target and return its v0 IR graph
    /// fragment. Implementations must not evaluate unrelated dynamic targets.
    async fn render_target_json(
        &self,
        project_dir: &Path,
        target_name: &str,
        context: &DynamicContext,
    ) -> anyhow::Result<String>;
    /// Emit the full discovery envelope JSON for every pipeline in the repo:
    /// `{"schema_version": "...", "pipelines": [{slug, name, allow_manual,
    /// triggers, definition}, ...]}`. Returned verbatim from the DSL runtime so
    /// the backend's pipeline discovery can consume it directly.
    async fn registry_json(&self, project_dir: &Path) -> anyhow::Result<String>;
}

/// Return an appropriate [`DslEngine`] for the given language.
///
/// # Errors
///
/// Returns an error if the required system runtime (`python3`, `node`/`bun`)
/// is not found on PATH.
pub fn engine_for(lang: DslLanguage) -> anyhow::Result<Box<dyn DslEngine>> {
    match lang {
        DslLanguage::Python => {
            let engine = python_engine::SubprocessPythonEngine::new()?;
            Ok(Box::new(engine))
        }
        DslLanguage::TypeScript => {
            let engine = ts_engine::SubprocessTsEngine::new()?;
            Ok(Box::new(engine))
        }
    }
}
