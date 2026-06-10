//! High-level VM orchestrator.

use std::sync::Arc;

use anyhow::Result;
use tracing::{instrument, warn};

use crate::backend::VmBackend;
use crate::registry::ImageRegistry;
use crate::types::{
    Action, CachingPolicy, ExecutionResult, ImageSource, OutputSink, SnapshotLabel, VmConfig,
};

/// High-level orchestrator that drives the VM lifecycle.
///
/// `HmVm` composes a [`VmBackend`] with an [`ImageRegistry`] to provide
/// cache-aware execution: if a cached snapshot already exists for a given
/// caching key the expensive create-inject-exec cycle is skipped entirely.
#[derive(Debug)]
pub struct HmVm {
    backend: Arc<dyn VmBackend>,
    registry: ImageRegistry,
    config: VmConfig,
}

impl HmVm {
    /// Create a new orchestrator from the given backend, registry and config.
    pub fn new(backend: Arc<dyn VmBackend>, registry: ImageRegistry, config: VmConfig) -> Self {
        Self {
            backend,
            registry,
            config,
        }
    }

    /// Execute an [`Action`] inside a VM, obeying the given [`CachingPolicy`].
    ///
    /// # Cache behaviour
    ///
    /// When the policy is [`CachingPolicy::Cache`] the registry is consulted
    /// first. A cache hit that still exists in the backend returns immediately.
    /// On a successful (exit-code 0) execution the resulting snapshot is stored
    /// in the registry; evicted entries are cleaned up in the backend.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend fails to create, restore, inject, or
    /// execute. Best-effort cleanup is performed even on failure paths.
    #[instrument(skip(self, action, sink), fields(cmd = %action.cmd))]
    pub async fn execute(
        &self,
        action: Action,
        policy: CachingPolicy,
        sink: &dyn OutputSink,
    ) -> Result<ExecutionResult> {
        // 1. Cache check
        if let CachingPolicy::Cache { ref key } = policy
            && let Some(snap) = self.registry.get(key)
        {
            if self.backend.snapshot_exists(&snap).await? {
                return Ok(ExecutionResult {
                    exit_code: 0,
                    snapshot: Some(snap),
                    cached: true,
                });
            }
            let _ = self.registry.invalidate(key);
        }

        // 2. Create or restore VM
        let mut vm = match &action.source {
            ImageSource::Image(image) => self.backend.create(image, &self.config).await?,
            ImageSource::Snapshot(snap) => self.backend.restore(snap, &self.config).await?,
        };

        let result = self.run_in_vm(&mut *vm, &action, &policy, sink).await;

        // Always destroy the VM, even on error.
        vm.destroy().await.ok();

        result
    }

    /// Inner lifecycle: inject, exec, snapshot. Separated so the caller
    /// can guarantee `vm.destroy()` runs regardless of outcome.
    async fn run_in_vm(
        &self,
        vm: &mut dyn crate::backend::Vm,
        action: &Action,
        policy: &CachingPolicy,
        sink: &dyn OutputSink,
    ) -> Result<ExecutionResult> {
        // 3. Inject workspace
        if let Some(ref host_path) = action.inject {
            vm.inject(host_path, &action.working_dir).await?;
        }

        // 4. Execute command (with optional timeout)
        let exec_fut = vm.exec(&action.cmd, &action.env, &action.working_dir, sink);
        let exit_code = if let Some(timeout) = action.timeout {
            match tokio::time::timeout(timeout, exec_fut).await {
                Ok(result) => result?,
                Err(_) => anyhow::bail!("command timed out after {timeout:?}"),
            }
        } else {
            exec_fut.await?
        };

        // 5. Snapshot and cache on success
        let snapshot = if exit_code == 0 {
            let label = match policy {
                CachingPolicy::Cache { key } => SnapshotLabel::Cached(key.clone()),
                CachingPolicy::None => SnapshotLabel::Ephemeral,
            };
            let snap = vm.snapshot(&label).await?;

            if let CachingPolicy::Cache { key } = policy {
                let evicted = self.registry.put(key, &snap);
                for old in &evicted {
                    if let Err(e) = self.backend.remove_snapshot(old).await {
                        warn!(snapshot = %old, error = %e, "failed to remove evicted snapshot");
                    }
                }
            }

            Some(snap)
        } else {
            None
        };

        Ok(ExecutionResult {
            exit_code,
            snapshot,
            cached: false,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::backend::Vm;
    use crate::types::{NullSink, SnapshotId};

    use std::path::Path;
    use std::sync::Mutex;

    use async_trait::async_trait;

    // ------------------------------------------------------------------ //
    // Mock backend + VM                                                    //
    // ------------------------------------------------------------------ //

    #[derive(Debug, Clone)]
    struct MockBackend {
        calls: Arc<Mutex<Vec<String>>>,
        /// Exit code that `MockVm::exec` will return.
        exit_code: i32,
        /// Whether `snapshot_exists` should return true.
        snapshot_exists: bool,
    }

    impl MockBackend {
        fn new(exit_code: i32, snapshot_exists: bool) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                exit_code,
                snapshot_exists,
            }
        }
    }

    #[async_trait]
    impl VmBackend for MockBackend {
        async fn create(&self, image: &str, _config: &VmConfig) -> Result<Box<dyn Vm>> {
            self.calls
                .lock()
                .map_or_else(|_| {}, |mut c| c.push(format!("create:{image}")));
            Ok(Box::new(MockVm {
                calls: Arc::clone(&self.calls),
                exit_code: self.exit_code,
            }))
        }

        async fn restore(&self, snapshot: &SnapshotId, _config: &VmConfig) -> Result<Box<dyn Vm>> {
            self.calls
                .lock()
                .map_or_else(|_| {}, |mut c| c.push(format!("restore:{snapshot}")));
            Ok(Box::new(MockVm {
                calls: Arc::clone(&self.calls),
                exit_code: self.exit_code,
            }))
        }

        async fn snapshot_exists(&self, snapshot: &SnapshotId) -> Result<bool> {
            self.calls.lock().map_or_else(
                |_| {},
                |mut c| c.push(format!("snapshot_exists:{snapshot}")),
            );
            Ok(self.snapshot_exists)
        }

        async fn remove_snapshot(&self, snapshot: &SnapshotId) -> Result<()> {
            self.calls.lock().map_or_else(
                |_| {},
                |mut c| c.push(format!("remove_snapshot:{snapshot}")),
            );
            Ok(())
        }
    }

    struct MockVm {
        calls: Arc<Mutex<Vec<String>>>,
        exit_code: i32,
    }

    #[async_trait]
    impl Vm for MockVm {
        async fn inject(&self, host_path: &Path, guest_path: &str) -> Result<()> {
            self.calls.lock().map_or_else(
                |_| {},
                |mut c| c.push(format!("inject:{}:{guest_path}", host_path.display())),
            );
            Ok(())
        }

        async fn exec(
            &self,
            cmd: &str,
            _env: &[(String, String)],
            _working_dir: &str,
            _sink: &dyn OutputSink,
        ) -> Result<i32> {
            self.calls
                .lock()
                .map_or_else(|_| {}, |mut c| c.push(format!("exec:{cmd}")));
            Ok(self.exit_code)
        }

        async fn snapshot(&mut self, label: &SnapshotLabel) -> Result<SnapshotId> {
            let label = match label {
                SnapshotLabel::Ephemeral => "ephemeral".to_string(),
                SnapshotLabel::Cached(key) => key.clone(),
            };
            self.calls
                .lock()
                .map_or_else(|_| {}, |mut c| c.push(format!("snapshot:{label}")));
            Ok(SnapshotId::new(format!("snap-{label}")))
        }

        async fn destroy(&mut self) -> Result<()> {
            self.calls
                .lock()
                .map_or_else(|_| {}, |mut c| c.push("destroy".into()));
            Ok(())
        }
    }

    // ------------------------------------------------------------------ //
    // Helpers                                                              //
    // ------------------------------------------------------------------ //

    fn open_temp_registry(capacity: u64) -> (ImageRegistry, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let db = dir.path().join("registry.db");
        let capacity = std::num::NonZeroU64::new(capacity).expect("capacity must be non-zero");
        let reg = ImageRegistry::open(&db, capacity).expect("failed to open registry");
        (reg, dir)
    }

    fn make_action() -> Action {
        Action {
            source: ImageSource::Image("alpine:latest".into()),
            cmd: "echo hello".into(),
            env: vec![],
            working_dir: "/work".into(),
            timeout: None,
            inject: Some(std::path::PathBuf::from("/host/src")),
        }
    }

    fn calls(backend: &MockBackend) -> Vec<String> {
        backend.calls.lock().map_or_else(|_| vec![], |c| c.clone())
    }

    // ------------------------------------------------------------------ //
    // Tests                                                                //
    // ------------------------------------------------------------------ //

    #[tokio::test]
    async fn cache_miss_creates_executes_and_snapshots() {
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let result = hm
            .execute(
                make_action(),
                CachingPolicy::Cache {
                    key: "step-1".into(),
                },
                &NullSink,
            )
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 0);
        assert!(!result.cached);
        assert!(result.snapshot.is_some());

        let log = calls(&backend);
        assert!(log.iter().any(|c| c.starts_with("create:")));
        assert!(log.iter().any(|c| c.starts_with("inject:")));
        assert!(log.iter().any(|c| c.starts_with("exec:")));
        assert!(log.iter().any(|c| c.starts_with("snapshot:")));
        assert!(log.iter().any(|c| c == "destroy"));
    }

    #[tokio::test]
    async fn cache_hit_skips_execution() {
        let backend = MockBackend::new(0, true);
        let (registry, _dir) = open_temp_registry(10);

        // Pre-populate the registry.
        registry.put("step-1", &SnapshotId::new("cached-snap"));

        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let result = hm
            .execute(
                make_action(),
                CachingPolicy::Cache {
                    key: "step-1".into(),
                },
                &NullSink,
            )
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 0);
        assert!(result.cached);
        assert_eq!(result.snapshot, Some(SnapshotId::new("cached-snap")));

        let log = calls(&backend);
        // Only snapshot_exists should have been called -- no create, exec, etc.
        assert!(log.iter().any(|c| c.starts_with("snapshot_exists:")));
        assert!(!log.iter().any(|c| c.starts_with("create:")));
        assert!(!log.iter().any(|c| c.starts_with("exec:")));
    }

    #[tokio::test]
    async fn no_cache_policy_does_not_store() {
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let result = hm
            .execute(make_action(), CachingPolicy::None, &NullSink)
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 0);
        assert!(!result.cached);

        // Exec should have run.
        let log = calls(&backend);
        assert!(log.iter().any(|c| c.starts_with("exec:")));

        // Registry should be empty -- no caching performed.
        assert!(hm.registry.is_empty());
    }

    #[tokio::test]
    async fn nonzero_exit_does_not_cache() {
        let backend = MockBackend::new(1, false);
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let result = hm
            .execute(
                make_action(),
                CachingPolicy::Cache {
                    key: "step-fail".into(),
                },
                &NullSink,
            )
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 1);
        assert!(!result.cached);
        assert!(result.snapshot.is_none());

        let log = calls(&backend);
        // Exec should have run but no snapshot taken.
        assert!(log.iter().any(|c| c.starts_with("exec:")));
        assert!(!log.iter().any(|c| c.starts_with("snapshot:")));

        // Registry should still be empty.
        assert!(hm.registry.is_empty());
    }
}
