//! Embedded DSL engine: run Python/TypeScript pipeline definitions via Wasmtime.

use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;

pub mod detect;

// Feature-gated modules — added as engines are implemented:
// mod wasm_runtime;   (embedded-python | embedded-typescript)
// mod runtime_cache;  (embedded-python | embedded-typescript)
// mod python_engine;  (embedded-python)
// mod js_engine;      (embedded-typescript)
// mod ts_preprocess;  (embedded-typescript)

/// The language a DSL pipeline is written in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DslLanguage {
    /// `CPython` compiled to WASI.
    Python,
    /// TypeScript transpiled to JS, run via `QuickJS`-WASI.
    TypeScript,
}

/// Minimal metadata extracted from a DSL pipeline module.
#[derive(Debug, Clone, Deserialize)]
pub struct PipelineMeta {
    /// URL-safe identifier (e.g. `"deploy-api"`).
    pub slug: String,
    /// Human-readable display name.
    pub name: String,
}

/// Trait implemented by each language-specific engine.
#[async_trait]
pub trait DslEngine: Send + Sync {
    /// Discover all pipeline definitions under `project_dir`.
    async fn list_pipelines(&self, project_dir: &Path) -> anyhow::Result<Vec<PipelineMeta>>;

    /// Render a single pipeline to its JSON IR.
    async fn render_pipeline_json(
        &self,
        project_dir: &Path,
        slug: &str,
    ) -> anyhow::Result<String>;
}

/// Return an appropriate [`DslEngine`] for the given language.
///
/// # Errors
///
/// Returns an error if the requested language feature was not compiled in.
#[allow(clippy::unused_async)] // Will await engine constructors once implemented.
pub async fn engine_for(lang: DslLanguage) -> anyhow::Result<Box<dyn DslEngine>> {
    match lang {
        DslLanguage::Python => {
            #[cfg(feature = "embedded-python")]
            {
                let _ = lang; // silence unused-variable warning in stub
                anyhow::bail!("Python engine not yet implemented");
            }
            #[cfg(not(feature = "embedded-python"))]
            {
                anyhow::bail!(
                    "embedded-python feature is not enabled; \
                     rebuild with `--features embedded-python`"
                );
            }
        }
        DslLanguage::TypeScript => {
            #[cfg(feature = "embedded-typescript")]
            {
                let _ = lang;
                anyhow::bail!("TypeScript engine not yet implemented");
            }
            #[cfg(not(feature = "embedded-typescript"))]
            {
                anyhow::bail!(
                    "embedded-typescript feature is not enabled; \
                     rebuild with `--features embedded-typescript`"
                );
            }
        }
    }
}
