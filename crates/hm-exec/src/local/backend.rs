//! [`LocalDockerBackend`]: runs the build in-process via the Docker DAG scheduler.

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::local::{DockerRunner, RunnerRegistry};
use crate::{BackendHandle, Capabilities, ExecutionBackend, Result, RunRequest};

/// Runs the build locally via the in-process Docker DAG scheduler.
///
/// Constructed once and reused across multiple `start` calls.
/// `parallelism` controls the maximum number of concurrently running step
/// chains; the scheduler serialises within each chain regardless.
#[derive(Debug)]
pub struct LocalDockerBackend {
    parallelism: usize,
    registry: Arc<RunnerRegistry>,
}

impl LocalDockerBackend {
    /// Build a backend with the default Docker runner registered.
    ///
    /// `parallelism` = max concurrent step chains. `0` is coerced to `1`
    /// by the scheduler.
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
impl ExecutionBackend for LocalDockerBackend {
    fn name(&self) -> &'static str {
        "local-docker"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::local()
    }

    async fn start(&self, req: RunRequest) -> Result<BackendHandle> {
        let (tx, rx) = mpsc::channel(1024);
        let cancel = CancellationToken::new();
        let registry = self.registry.clone();
        let parallelism = self.parallelism;
        let token = cancel.clone();
        let join = tokio::spawn(async move {
            crate::local::run(
                req.plan.graph,
                req.repo_root,
                req.pipeline_slug,
                parallelism,
                registry,
                tx,
                token,
            )
            .await
        });
        Ok(BackendHandle::spawn(rx, cancel, join))
    }
}
