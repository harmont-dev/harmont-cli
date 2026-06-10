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
    /// the default runner. The orchestrator handle is also returned so the
    /// run loop can drain its deferred eviction queue once the DAG has
    /// finished.
    fn build_registry(&self) -> Result<(RunnerRegistry, Arc<HmVm>)> {
        let cache_dir = hm_util::dirs::harmont_cache_dir().ok_or_else(|| {
            BackendError::Local("cannot resolve the Harmont cache directory".into())
        })?;
        let registry = ImageRegistry::open(&cache_dir.join("registry.db"), REGISTRY_CAPACITY)
            .map_err(|e| BackendError::Local(format!("opening snapshot registry: {e:#}")))?;

        let config = VmConfig {
            memory_mib: Some(8192),
            disk_size_gb: Some(10),
            ..Default::default()
        };

        let hmvm = Arc::new(HmVm::new(Arc::clone(&self.vm_backend), registry, config));

        let mut runners = RunnerRegistry::new();
        runners.register(Arc::new(VmRunner::new(Arc::clone(&hmvm))), true);
        Ok((runners, hmvm))
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
        let (registry, hmvm) = self.build_registry()?;

        // Best-effort, detached GC of aged snapshot images in the
        // *run-scoped* namespaces only:
        // - `harmont-ephemeral`: uncached snapshots whose run died before
        //   its end-of-run cleanup.
        // - `ephemeral`: the EXACT legacy repo pre-fix versions used for
        //   uncached snapshots (`ephemeral:latest`, `ephemeral:<64hex>`).
        //   Deliberately not a glob — a wildcard like `ephemeral-*` would
        //   force-remove unrelated user images.
        // These tags are created and removed within a single run, so any
        // image older than the 24h floor is residue of a dead run; the
        // floor also keeps a starting run from deleting a concurrent run's
        // freshly committed, still-in-use ephemeral images.
        //
        // `harmont-cache/*` is deliberately NOT swept here. The registry DB
        // is per-user (`hm_util::dirs::harmont_cache_dir`) while the Docker
        // daemon can be shared across users and CI jobs, so absence from
        // THIS registry does not prove a cache image is orphaned — sweeping
        // would delete other registries' live caches and silently force
        // their cached steps to re-run. It would also race deferred LRU
        // eviction: an evicted row vanishes from SQLite the moment `put`
        // commits, while in-flight steps (ours or a concurrent process's)
        // may still restore from the evicted tag until their DAG drains —
        // a window no registry-based liveness check can observe. Crash
        // residue under `harmont-cache/*` is bounded by the registry
        // capacity and reclaimable via `hm cache clean` plus `docker rmi`.
        // Failures are logged and never block or fail the run.
        let gc_vm = Arc::clone(&hmvm);
        tokio::spawn(async move {
            #[allow(
                clippy::duration_suboptimal_units,
                reason = "from_hours is nightly-only"
            )]
            const GC_AGE: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
            for reference in ["harmont-ephemeral", "ephemeral"] {
                match gc_vm.gc_orphaned_snapshots(reference, GC_AGE).await {
                    Ok(0) => {}
                    Ok(n) => tracing::debug!(reference, removed = n, "snapshot GC removed images"),
                    Err(e) => tracing::warn!(reference, error = %e, "snapshot GC failed"),
                }
            }
        });
        let registry = Arc::new(registry);
        let (tx, rx) = mpsc::channel(1024);
        let cancel = CancellationToken::new();
        let parallelism = self.parallelism;
        let token = cancel.clone();
        let run_vm = Arc::clone(&hmvm);
        let join = tokio::spawn(async move {
            let result = crate::local::run(
                req.plan.graph,
                req.repo_root,
                req.pipeline_slug,
                parallelism,
                registry,
                tx,
                token,
                Some(run_vm),
            )
            .await;
            // Snapshot images evicted from the registry during the run are
            // only removed now, strictly after every step has finished: an
            // in-flight step may restore from an evicted tag long after the
            // eviction (cache-hit outcomes propagate only the tag).
            hmvm.cleanup_deferred_evictions().await;
            result
        });
        Ok(BackendHandle::spawn(rx, cancel, join))
    }
}
