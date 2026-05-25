use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;

pub mod detect;
pub mod python_engine;

#[allow(dead_code)] // Items used by engine modules added in Tasks 5-7.
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

#[async_trait]
pub trait DslEngine: Send + Sync {
    async fn list_pipelines(&self, project_dir: &Path) -> anyhow::Result<Vec<PipelineMeta>>;
    async fn render_pipeline_json(
        &self,
        project_dir: &Path,
        slug: &str,
    ) -> anyhow::Result<String>;
}

/// Placeholder — will be fully implemented when engines are added in Tasks 5-7.
///
/// # Errors
///
/// Always returns an error until engine modules are wired in Tasks 5-7.
pub fn engine_for(_lang: DslLanguage) -> anyhow::Result<Box<dyn DslEngine>> {
    anyhow::bail!("DSL engines not yet wired — Tasks 5-7 pending")
}
