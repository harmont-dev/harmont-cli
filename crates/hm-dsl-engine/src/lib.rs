use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;

pub mod detect;
pub mod lower;
pub mod step_chain;
mod python_engine;

pub use python_engine::{SubprocessPythonEngine, engine as python_engine};

mod bundled_sources;

#[derive(Debug, Clone, Deserialize)]
pub struct PipelineMeta {
    pub slug: String,
    pub name: String,
}

#[async_trait]
pub trait DslEngine: Send + Sync {
    async fn list_pipelines(&self, project_dir: &Path) -> anyhow::Result<Vec<PipelineMeta>>;
    async fn render_pipeline_json(&self, project_dir: &Path, slug: &str) -> anyhow::Result<String>;
    /// Emit the full discovery envelope JSON for every pipeline in the repo:
    /// `{"schema_version": "...", "pipelines": [{slug, name, allow_manual,
    /// triggers, definition}, ...]}`. Returned verbatim from the DSL runtime so
    /// the backend's pipeline discovery can consume it directly.
    async fn registry_json(&self, project_dir: &Path) -> anyhow::Result<String>;
}
