//! Local Docker-backed executor.
use std::sync::Arc;

use anyhow::Result;
use hm_render::OutputRenderer;

use crate::executor::{Executor, Rendered};
use crate::runner::{RunnerRegistry, docker::DockerRunner};

fn decode_plan_to_wire(bytes: &[u8]) -> anyhow::Result<hm_pipeline_ir::PipelineGraph> {
    serde_json::from_slice(bytes).map_err(|e| anyhow::anyhow!("decode pipeline JSON: {e}"))
}

/// Runs the build locally via the Docker orchestrator.
#[derive(Debug)]
pub struct LocalExecutor {
    pub parallelism: usize,
    pub registry: Arc<RunnerRegistry>,
}

impl LocalExecutor {
    /// Construct a `LocalExecutor` with a default `DockerRunner` registry.
    ///
    /// `parallelism` is the maximum number of steps that may run concurrently.
    /// Pass `0` to use the number of logical CPUs (same default as `hm run`).
    #[must_use]
    pub fn new(parallelism: usize) -> Self {
        let mut registry = RunnerRegistry::new();
        registry.register(Arc::new(DockerRunner), true);
        Self {
            parallelism,
            registry: Arc::new(registry),
        }
    }
}

#[async_trait::async_trait]
impl Executor for LocalExecutor {
    async fn execute(&self, plan: Rendered, output: Box<dyn OutputRenderer>) -> Result<i32> {
        let graph = decode_plan_to_wire(plan.ir_json.as_bytes())?;
        crate::orchestrator::run(
            graph,
            plan.repo_root,
            self.parallelism,
            self.registry.clone(),
            output,
        )
        .await
    }
}
