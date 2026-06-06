//! Execution backends for `hm run`: local (Docker) and cloud share one
//! `Executor` trait + one `hm_render` renderer set.
use std::path::PathBuf;

use anyhow::Result;
use hm_render::OutputRenderer;

mod local;
pub use local::LocalExecutor;

/// A rendered pipeline ready to execute: the repo root (for source), the
/// pipeline slug, and the v0 IR JSON.
#[derive(Debug, Clone)]
pub struct Rendered {
    pub repo_root: PathBuf,
    pub slug: String,
    pub ir_json: String,
}

/// A run backend. `execute` consumes the rendered pipeline and an output
/// renderer, drives the run, and returns the process exit code.
#[async_trait::async_trait]
pub trait Executor: Send + Sync {
    async fn execute(&self, plan: Rendered, output: Box<dyn OutputRenderer>) -> Result<i32>;
}
