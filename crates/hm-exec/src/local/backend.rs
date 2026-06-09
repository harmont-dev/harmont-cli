//! [`LocalBackend`]: runs the build in-process via the DAG scheduler.
//!
//! Each step is executed inside a lightweight VM by the [`VmRunner`], which
//! drives the [`hm_vm`] subsystem. The VM backend (Docker, etc.) is injected;
//! snapshot caching is owned by `hm-vm`'s [`hm_vm::ImageRegistry`].

use std::sync::Arc;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use hm_vm::{HmVm, ImageRegistry, VmBackend, VmConfig};

use crate::local::{RunnerRegistry, VmRunner};
use crate::{BackendError, BackendHandle, Capabilities, ExecutionBackend, Result, RunRequest};

/// Number of cached snapshots the image registry retains before evicting
/// least-recently-used entries.
const REGISTRY_CAPACITY: u64 = 64;

/// Runs the build locally via the in-process DAG scheduler, executing each
/// step inside a VM provided by the injected [`hm_vm::VmBackend`].
///
/// Constructed once and reused across multiple `start` calls.
/// `parallelism` controls the maximum number of concurrently running step
/// chains; the scheduler serialises within each chain regardless.
#[derive(Debug)]
pub struct LocalBackend {
    parallelism: usize,
    vm_backend: Arc<dyn VmBackend>,
}

impl LocalBackend {
    /// Build a backend that executes steps on the given [`hm_vm::VmBackend`].
    ///
    /// `parallelism` = max concurrent step chains. `0` is coerced to `1`
    /// by the scheduler.
    #[must_use]
    pub fn new(parallelism: usize, vm_backend: Arc<dyn VmBackend>) -> Self {
        Self {
            parallelism,
            vm_backend,
        }
    }

    /// Build the runner registry, constructing the [`HmVm`] orchestrator
    /// (VM backend + snapshot registry) and registering the [`VmRunner`] as
    /// the default runner.
    fn build_registry(&self) -> Result<RunnerRegistry> {
        let cache_dir = hm_util::dirs::hm_cache_dir().ok_or_else(|| {
            BackendError::Local("cannot resolve the Harmont cache directory".into())
        })?;
        let registry = ImageRegistry::open(&cache_dir.join("registry.db"), REGISTRY_CAPACITY)
            .map_err(|e| BackendError::Local(format!("opening snapshot registry: {e:#}")))?;

        let config = VmConfig {
            memory_mib: Some(8192),
            disk_size_gb: Some(10),
            ..Default::default()
        };

        let hmvm = HmVm::new(Arc::clone(&self.vm_backend), registry, config);

        let mut runners = RunnerRegistry::new();
        runners.register(Arc::new(VmRunner::new(Arc::new(hmvm))), true);
        Ok(runners)
    }
}

#[async_trait::async_trait]
impl ExecutionBackend for LocalBackend {
    fn name(&self) -> &'static str {
        "local"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities::local()
    }

    async fn start(&self, req: RunRequest) -> Result<BackendHandle> {
        let registry = Arc::new(self.build_registry()?);
        let (tx, rx) = mpsc::channel(1024);
        let cancel = CancellationToken::new();
        let parallelism = self.parallelism;
        let keep_going = req.options.keep_going;
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
                keep_going,
            )
            .await
        });
        Ok(BackendHandle::spawn(rx, cancel, join))
    }
}
