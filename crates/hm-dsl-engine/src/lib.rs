use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;

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

#[async_trait]
pub trait DslEngine: Send + Sync {
    async fn list_pipelines(&self, project_dir: &Path) -> anyhow::Result<Vec<PipelineMeta>>;
    async fn render_pipeline_json(&self, project_dir: &Path, slug: &str) -> anyhow::Result<String>;
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
