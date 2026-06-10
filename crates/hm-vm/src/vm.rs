//! High-level VM orchestrator.

use std::sync::Arc;

use anyhow::Result;
use tokio_util::sync::CancellationToken;
use tracing::{instrument, warn};

use crate::backend::VmBackend;
use crate::registry::ImageRegistry;
use crate::types::{
    Action, CachingPolicy, ExecutionResult, ImageSource, OutputSink, SnapshotId, SnapshotLabel,
    VmConfig,
};

/// Exit code reported when execution is cut short by cooperative
/// cancellation (Ctrl-C, sibling failure, per-step timeout). Mirrors the
/// conventional 128+SIGINT shell encoding; the scheduler maps it to a
/// `Canceled` step status.
const CANCELLED_EXIT_CODE: i32 = 130;

/// The [`ExecutionResult`] returned when a step is cancelled before or
/// during execution: nothing was committed, nothing was cached.
const fn cancelled_result() -> ExecutionResult {
    ExecutionResult {
        exit_code: CANCELLED_EXIT_CODE,
        snapshot: None,
        cached: false,
        ephemeral_snapshot: false,
    }
}

/// High-level orchestrator that drives the VM lifecycle.
///
/// `HmVm` composes a [`VmBackend`] with an [`ImageRegistry`] to provide
/// cache-aware execution: if a cached snapshot already exists for a given
/// caching key the expensive create-exec cycle is skipped entirely.
#[derive(Debug)]
pub struct HmVm {
    backend: Arc<dyn VmBackend>,
    registry: ImageRegistry,
    config: VmConfig,
    /// Snapshots (plus optional legacy workspace dirs) evicted from the
    /// registry during this run. Their backend images are NOT removed at
    /// eviction time: an in-flight step may still hold the evicted tag as
    /// its `parent_snapshot` and restore from it later (cache-hit outcomes
    /// propagate only the tag). Removal is deferred to
    /// [`Self::cleanup_deferred_evictions`], which the local backend calls
    /// strictly after the whole DAG has drained.
    deferred_evictions: std::sync::Mutex<Vec<(crate::types::SnapshotId, Option<String>)>>,
}

impl HmVm {
    /// Create a new orchestrator from the given backend, registry and config.
    pub fn new(backend: Arc<dyn VmBackend>, registry: ImageRegistry, config: VmConfig) -> Self {
        Self {
            backend,
            registry,
            config,
            deferred_evictions: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Check whether a cached result exists for `key` without executing anything.
    ///
    /// Returns `Some(result)` on a valid hit (snapshot still exists in the
    /// backend). Invalidates stale entries as a side-effect. Returns `None`
    /// on miss.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend's `snapshot_exists` check fails.
    pub async fn peek_cache(&self, key: &str) -> Result<Option<ExecutionResult>> {
        self.check_cache(key).await
    }

    /// Single implementation of the cache-hit check shared by
    /// [`Self::peek_cache`] and [`Self::execute`].
    ///
    /// A hit requires only a registry row plus a live backend snapshot: the
    /// cache stores system state (what `docker commit` captures) and nothing
    /// else. A stale row (image removed out-of-band, e.g. `docker rmi`) is
    /// invalidated with a compare-and-delete so a concurrently re-inserted
    /// fresh entry is never destroyed; any legacy workspace directory the
    /// row still referenced is reaped lazily.
    async fn check_cache(&self, key: &str) -> Result<Option<ExecutionResult>> {
        let Some((snap, _legacy_ws)) = self.registry.get_with_workspace(key) else {
            return Ok(None);
        };
        if self.backend.snapshot_exists(&snap).await? {
            return Ok(Some(ExecutionResult {
                exit_code: 0,
                snapshot: Some(snap),
                cached: true,
                ephemeral_snapshot: false,
            }));
        }
        // The image is already gone, so there is nothing to remove in the
        // backend -- only the registry row (and any legacy workspace dir).
        warn!(key, snapshot = %snap, "cached snapshot missing from backend; invalidating entry");
        if let Some(Some(legacy_ws)) = self.registry.invalidate_if(key, &snap) {
            tokio::task::spawn_blocking(move || std::fs::remove_dir_all(legacy_ws).ok())
                .await
                .ok();
        }
        Ok(None)
    }

    /// Remove backend images for registry entries evicted during this run.
    ///
    /// Must be called strictly after every step of the run has finished
    /// (the local backend invokes it once the scheduler's DAG has fully
    /// drained), so no in-flight step can still restore from an evicted
    /// tag.
    ///
    /// A tag that has been re-registered since its eviction is skipped:
    /// the same key may have been rebuilt later in this run, or a
    /// concurrent process may have re-inserted it, and backend re-tagging
    /// means the tag now names the *fresh* image — removing it would
    /// destroy a live cache entry. (A narrow cross-process window remains:
    /// a concurrent run that observed the tag before the eviction — as its
    /// own cache hit, whose children then carry the tag as their
    /// `parent_snapshot` — and restores from it after this cleanup gets a
    /// hard "no such image" restore failure and the step FAILS; the
    /// `parent_snapshot` restore path never goes through `check_cache`, so
    /// this does not degrade gracefully into re-execution. The stale row is
    /// invalidated on that process's next `check_cache` of the key. Closing
    /// the window entirely needs backend-level coordination between
    /// processes, which the registry cannot provide.)
    ///
    /// Legacy workspace directories riding on evicted rows are always
    /// reaped. Best-effort throughout: failures are logged, never
    /// propagated.
    pub async fn cleanup_deferred_evictions(&self) {
        let pending = std::mem::take(
            &mut *self
                .deferred_evictions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        for (snap, legacy_ws) in pending {
            if self.registry.contains_snapshot(&snap) {
                tracing::debug!(snapshot = %snap, "evicted tag was re-registered; keeping image");
            } else if let Err(e) = self.backend.remove_snapshot(&snap).await {
                warn!(snapshot = %snap, error = %e, "failed to remove evicted snapshot");
            }
            if let Some(ws_path) = legacy_ws {
                tokio::task::spawn_blocking(move || std::fs::remove_dir_all(ws_path).ok())
                    .await
                    .ok();
            }
        }
    }

    /// Remove `snapshot` from the backend unless the registry currently
    /// maps some key to it.
    ///
    /// Guarded twin of a bare `VmBackend::remove_snapshot` for reaping
    /// run-scoped (ephemeral or demoted-to-ephemeral) snapshots: if a
    /// concurrent run re-registered the same tag after this run marked it
    /// ephemeral — e.g. a `harmont-cache/*` tag demoted by a failed
    /// `registry.put` that another process has since successfully cached —
    /// removing the image would destroy a live cache entry. Best-effort:
    /// failures are logged, never propagated.
    pub async fn remove_snapshot_unless_registered(&self, snapshot: &SnapshotId) {
        if self.registry.contains_snapshot(snapshot) {
            tracing::debug!(snapshot = %snapshot, "snapshot was re-registered; keeping image");
        } else if let Err(e) = self.backend.remove_snapshot(snapshot).await {
            warn!(snapshot = %snapshot, error = %e, "failed to remove ephemeral snapshot");
        }
    }

    /// Execute an [`Action`] inside a VM, obeying the given [`CachingPolicy`].
    ///
    /// # Cache behaviour
    ///
    /// When the policy is [`CachingPolicy::Cache`] the registry is consulted
    /// first. A cache hit that still exists in the backend returns immediately.
    /// On a successful (exit-code 0) execution the resulting snapshot is stored
    /// in the registry; entries evicted by the insert are queued for removal
    /// via [`Self::cleanup_deferred_evictions`] once the run has drained.
    ///
    /// # Cancellation
    ///
    /// Cancellation is cooperative via `cancel`: when the token fires, the
    /// in-flight command is abandoned and the VM is destroyed — and that
    /// teardown (including the bind-mount ownership reclaim on native Linux
    /// Docker) is *awaited* before this future resolves. Callers must
    /// therefore never `select!`-drop this future on cancellation; awaiting
    /// it guarantees the workspace directory is safe to read or remove the
    /// moment it returns. A cancelled execution yields exit code 130 with
    /// no snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend fails to create, restore, or
    /// execute. Best-effort cleanup is performed even on failure paths.
    #[instrument(skip(self, action, sink, cancel), fields(cmd = %action.cmd))]
    pub async fn execute(
        &self,
        action: Action,
        policy: CachingPolicy,
        sink: &dyn OutputSink,
        cancel: &CancellationToken,
    ) -> Result<ExecutionResult> {
        // 1. Cache check. Callers typically `peek_cache` first; this second
        // check is deliberately retained -- it is cheap (one SQLite read +
        // one image list) and lets a concurrent fill between peek and
        // execute still count as a hit, at worst wasting one COW copy.
        if let CachingPolicy::Cache { ref key } = policy
            && let Some(result) = self.check_cache(key).await?
        {
            return Ok(result);
        }

        // 2. Create or restore the VM, bailing cooperatively if the run is
        // cancelled mid-boot (image pulls can take minutes). No container
        // handle exists yet on the cancel branch, so there is nothing to
        // tear down, and the workspace still holds only host-user-owned
        // files, so the caller may delete it immediately. (`biased` makes
        // an already-cancelled token win deterministically. Dropping a
        // creation request mid-roundtrip can, in a sub-millisecond window,
        // leave a never-started container behind on the daemon; it holds no
        // processes and writes nothing, so it cannot affect the workspace.)
        let create_fut = async {
            match &action.source {
                ImageSource::Image(image) => {
                    self.backend
                        .create(image, &self.config, action.workspace.as_ref())
                        .await
                }
                ImageSource::Snapshot(snap) => {
                    self.backend
                        .restore(snap, &self.config, action.workspace.as_ref())
                        .await
                }
            }
        };
        let mut vm = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(cancelled_result()),
            vm = create_fut => vm?,
        };

        let result = self
            .run_in_vm(&mut *vm, &action, &policy, sink, cancel)
            .await;

        // Always destroy the VM -- on success, error AND cancellation --
        // and await it: teardown reclaims bind-mount ownership and must
        // happen-before the caller touches or removes the workspace dir.
        vm.destroy().await.ok();

        result
    }

    /// Remove a snapshot from the backend store.
    ///
    /// Used to reap ephemeral (uncached) leaf snapshots once a run finishes —
    /// `CachingPolicy::None` commits a transient `ephemeral:*` image purely for
    /// downstream container lineage, and nothing in the registry ever evicts
    /// it. The scheduler reaps these explicitly at run end.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend fails to remove the snapshot.
    pub async fn remove_snapshot(&self, snapshot: &SnapshotId) -> Result<()> {
        self.backend.remove_snapshot(snapshot).await
    }

    /// Best-effort sweep of aged snapshot images that neither a registry
    /// row nor this run's deferred-eviction queue references.
    ///
    /// Intended for *run-scoped* tag namespaces (`harmont-ephemeral`, the
    /// legacy `ephemeral`), whose images are created and removed within a
    /// single run: an aged tag there is residue of a dead run. It must NOT
    /// be pointed at the shared cache namespace (`harmont-cache/*`) by
    /// anything that runs automatically: the registry DB is per-user while
    /// the backend (Docker daemon) can be shared, so a missing row does not
    /// prove an image is orphaned — another user's registry may still
    /// reference it. The `older_than` floor protects a concurrent run's
    /// freshly committed images that have not been registered yet.
    ///
    /// The keep predicate protects, in addition to live registry rows, any
    /// tag queued in [`Self::deferred_evictions`]: an LRU-evicted row is
    /// gone from `SQLite` immediately, but in-flight steps of this run may
    /// still restore from the evicted tag until the DAG drains, so it must
    /// survive any sweep that runs concurrently with the run.
    ///
    /// # Errors
    ///
    /// Returns an error only if the backend cannot be queried at all;
    /// per-image removal failures are logged and skipped.
    pub async fn gc_orphaned_snapshots(
        &self,
        reference: &str,
        older_than: std::time::Duration,
    ) -> Result<u64> {
        self.backend
            .gc_snapshots(reference, older_than, &|tag| {
                self.registry
                    .contains_snapshot(&SnapshotId::new(tag.to_owned()))
                    || self.is_eviction_deferred(tag)
            })
            .await
    }

    /// Whether `tag` is queued for deferred end-of-run removal — i.e. an
    /// in-flight step of this run may still restore from it even though its
    /// registry row is already gone.
    fn is_eviction_deferred(&self, tag: &str) -> bool {
        self.deferred_evictions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .any(|(snap, _)| snap.as_ref() == tag)
    }

    /// Inner lifecycle: exec, snapshot, register. Separated so the caller
    /// can guarantee `vm.destroy()` runs regardless of outcome.
    ///
    /// The workspace bind mount is never persisted: snapshots capture system
    /// state only, and workspace state is strictly run-scoped (owned by the
    /// caller's temp directories).
    async fn run_in_vm(
        &self,
        vm: &mut dyn crate::backend::Vm,
        action: &Action,
        policy: &CachingPolicy,
        sink: &dyn OutputSink,
        cancel: &CancellationToken,
    ) -> Result<ExecutionResult> {
        // 3. Execute command (with optional timeout), bailing cooperatively
        // on cancellation. Dropping the exec future abandons only the
        // output stream -- the in-container process may keep running until
        // the caller's `vm.destroy()` quiesces the container (the backend
        // tracks the interrupted exec and stops every process before
        // reclaiming workspace ownership).
        let exec_inner = vm.exec(&action.cmd, &action.env, &action.working_dir, sink);
        let exec_fut = async {
            if let Some(timeout) = action.timeout {
                match tokio::time::timeout(timeout, exec_inner).await {
                    Ok(result) => result,
                    Err(_) => anyhow::bail!("command timed out after {timeout:?}"),
                }
            } else {
                exec_inner.await
            }
        };
        let exit_code = tokio::select! {
            biased;
            () = cancel.cancelled() => return Ok(cancelled_result()),
            exit = exec_fut => exit?,
        };

        // 4. Snapshot and cache on success
        let (snapshot, ephemeral) = if exit_code == 0 {
            let (label, mut is_ephemeral) = match &policy {
                CachingPolicy::Cache { key } => (SnapshotLabel::Cached(key.clone()), false),
                CachingPolicy::None => (SnapshotLabel::Ephemeral, true),
            };
            let snap = vm.snapshot(&label).await?;

            if let CachingPolicy::Cache { key } = &policy {
                match self.registry.put(key, &snap) {
                    Ok(evicted) => {
                        // Do NOT remove evicted images here: a cache-hit
                        // outcome propagates only the snapshot tag to its
                        // children, and a child may restore from that tag
                        // long after a sibling's put evicts the row (the
                        // child can be parked on the parallelism semaphore).
                        // Queue the entries; the local backend drains them
                        // via `cleanup_deferred_evictions` strictly after
                        // the whole DAG has finished.
                        if !evicted.is_empty() {
                            self.deferred_evictions
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .extend(evicted);
                        }
                    }
                    Err(e) => {
                        // The snapshot exists but could not be registered.
                        // Demote it to ephemeral so the scheduler removes it
                        // after the run (same-run children still restore from
                        // it for lineage) instead of orphaning it forever.
                        warn!(
                            key,
                            error = %e,
                            "failed to record snapshot in registry; demoting to ephemeral"
                        );
                        is_ephemeral = true;
                    }
                }
            }

            (Some(snap), is_ephemeral)
        } else {
            (None, false)
        };

        Ok(ExecutionResult {
            exit_code,
            snapshot,
            cached: false,
            ephemeral_snapshot: ephemeral,
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::backend::Vm;
    use crate::types::{NullSink, SnapshotId, WorkspaceMount};

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
        /// Artificial latency for `MockVm::exec` (lets cancellation tests
        /// fire mid-execution).
        exec_delay: Option<std::time::Duration>,
        /// Aged snapshot tags the mock GC considers removable candidates.
        gc_candidates: Vec<String>,
    }

    impl MockBackend {
        fn new(exit_code: i32, snapshot_exists: bool) -> Self {
            Self {
                calls: Arc::new(Mutex::new(Vec::new())),
                exit_code,
                snapshot_exists,
                exec_delay: None,
                gc_candidates: Vec::new(),
            }
        }
    }

    #[async_trait]
    impl VmBackend for MockBackend {
        async fn create(
            &self,
            image: &str,
            _config: &VmConfig,
            _workspace: Option<&WorkspaceMount>,
        ) -> Result<Box<dyn Vm>> {
            self.calls
                .lock()
                .map_or_else(|_| {}, |mut c| c.push(format!("create:{image}")));
            Ok(Box::new(MockVm {
                calls: Arc::clone(&self.calls),
                exit_code: self.exit_code,
                exec_delay: self.exec_delay,
            }))
        }

        async fn restore(
            &self,
            snapshot: &SnapshotId,
            _config: &VmConfig,
            _workspace: Option<&WorkspaceMount>,
        ) -> Result<Box<dyn Vm>> {
            self.calls
                .lock()
                .map_or_else(|_| {}, |mut c| c.push(format!("restore:{snapshot}")));
            Ok(Box::new(MockVm {
                calls: Arc::clone(&self.calls),
                exit_code: self.exit_code,
                exec_delay: self.exec_delay,
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

        async fn gc_snapshots(
            &self,
            _reference: &str,
            _older_than: std::time::Duration,
            keep: &(dyn for<'a> Fn(&'a str) -> bool + Send + Sync),
        ) -> Result<u64> {
            let mut removed = 0;
            for tag in &self.gc_candidates {
                if keep(tag) {
                    continue;
                }
                self.calls
                    .lock()
                    .map_or_else(|_| {}, |mut c| c.push(format!("gc_remove:{tag}")));
                removed += 1;
            }
            Ok(removed)
        }
    }

    struct MockVm {
        calls: Arc<Mutex<Vec<String>>>,
        exit_code: i32,
        exec_delay: Option<std::time::Duration>,
    }

    #[async_trait]
    impl Vm for MockVm {
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
            if let Some(delay) = self.exec_delay {
                tokio::time::sleep(delay).await;
            }
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
            workspace: Some(WorkspaceMount {
                host_path: std::path::PathBuf::from("/host/src"),
                guest_path: "/work".into(),
            }),
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
                &CancellationToken::new(),
            )
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 0);
        assert!(!result.cached);
        assert!(result.snapshot.is_some());

        let log = calls(&backend);
        assert!(log.iter().any(|c| c.starts_with("create:")));
        assert!(log.iter().any(|c| c.starts_with("exec:")));
        assert!(log.iter().any(|c| c.starts_with("snapshot:")));
        assert!(log.iter().any(|c| c == "destroy"));
    }

    #[tokio::test]
    async fn cache_hit_skips_execution() {
        let backend = MockBackend::new(0, true);
        let (registry, _dir) = open_temp_registry(10);

        // Pre-populate the registry.
        registry
            .put("step-1", &SnapshotId::new("cached-snap"))
            .expect("put");

        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let result = hm
            .execute(
                make_action(),
                CachingPolicy::Cache {
                    key: "step-1".into(),
                },
                &NullSink,
                &CancellationToken::new(),
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
            .execute(
                make_action(),
                CachingPolicy::None,
                &NullSink,
                &CancellationToken::new(),
            )
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
                &CancellationToken::new(),
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

    #[tokio::test]
    async fn stale_entry_is_invalidated_and_step_reexecutes() {
        // Registry has a row, but the backend image is gone (e.g. an
        // out-of-band `docker rmi`). The step must re-execute and the stale
        // row must be replaced -- with no remove_snapshot call, since the
        // image is already absent.
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(10);
        registry
            .put("step-1", &SnapshotId::new("gone-snap"))
            .expect("put");

        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let result = hm
            .execute(
                make_action(),
                CachingPolicy::Cache {
                    key: "step-1".into(),
                },
                &NullSink,
                &CancellationToken::new(),
            )
            .await
            .expect("execute should succeed");

        assert!(!result.cached);
        assert_eq!(result.exit_code, 0);

        let log = calls(&backend);
        assert!(log.iter().any(|c| c.starts_with("exec:")));
        assert!(!log.iter().any(|c| c.starts_with("remove_snapshot:")));

        // The stale row was replaced by the fresh snapshot.
        assert_eq!(
            hm.registry.get("step-1"),
            Some(SnapshotId::new("snap-step-1"))
        );
    }

    #[tokio::test]
    async fn eviction_defers_snapshot_removal_until_cleanup() {
        // Capacity 1: caching a second key evicts the first. The evicted
        // image must NOT be removed inline -- an in-flight step may still
        // restore from the evicted tag -- only when the run has drained and
        // `cleanup_deferred_evictions` is invoked.
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(1);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        for key in ["step-a", "step-b"] {
            hm.execute(
                make_action(),
                CachingPolicy::Cache { key: key.into() },
                &NullSink,
                &CancellationToken::new(),
            )
            .await
            .expect("execute should succeed");
        }

        // No inline removal while steps could still be in flight.
        let log = calls(&backend);
        assert!(!log.iter().any(|c| c.starts_with("remove_snapshot:")));
        assert_eq!(hm.registry.len(), 1);
        assert!(hm.registry.get("step-b").is_some());

        // End of run: the evicted snapshot is removed now.
        // "step-a" ties with "step-b" on accessed_at; key ASC tie-break
        // makes the eviction deterministic.
        hm.cleanup_deferred_evictions().await;
        let log = calls(&backend);
        assert!(log.iter().any(|c| c == "remove_snapshot:snap-step-a"));
        assert!(!log.iter().any(|c| c == "remove_snapshot:snap-step-b"));
    }

    #[tokio::test]
    async fn deferred_eviction_skips_reregistered_snapshot() {
        // An evicted tag that is re-inserted before cleanup (same key
        // rebuilt later this run, or a concurrent process) now names the
        // fresh image; removing it would destroy a live cache entry.
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(1);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        for key in ["step-a", "step-b"] {
            hm.execute(
                make_action(),
                CachingPolicy::Cache { key: key.into() },
                &NullSink,
                &CancellationToken::new(),
            )
            .await
            .expect("execute should succeed");
        }
        // "step-a" was evicted and its removal deferred. Re-register the
        // same snapshot tag (as a concurrent process or a later rebuild of
        // the same key would after Docker re-tagging). The sleep gives the
        // re-inserted row a strictly newer accessed_at so the capacity-1
        // eviction removes "step-b", not the fresh row (timestamps have
        // one-second granularity).
        std::thread::sleep(std::time::Duration::from_secs(1));
        hm.registry
            .put("step-a", &SnapshotId::new("snap-step-a"))
            .expect("put");

        hm.cleanup_deferred_evictions().await;

        // The re-registered tag survives cleanup: it now names a live
        // cache entry's image.
        let log = calls(&backend);
        assert!(!log.iter().any(|c| c == "remove_snapshot:snap-step-a"));
        assert_eq!(
            hm.registry.get("step-a"),
            Some(SnapshotId::new("snap-step-a"))
        );
    }

    #[tokio::test]
    async fn remove_unless_registered_guards_live_entries() {
        // A demoted-to-ephemeral `harmont-cache/*` tag that a concurrent
        // run re-registered must survive the scheduler's ephemeral
        // cleanup; an unregistered tag must be removed.
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        hm.registry
            .put("step-a", &SnapshotId::new("snap-live"))
            .expect("put");

        hm.remove_snapshot_unless_registered(&SnapshotId::new("snap-live"))
            .await;
        hm.remove_snapshot_unless_registered(&SnapshotId::new("snap-orphan"))
            .await;

        let log = calls(&backend);
        assert!(!log.iter().any(|c| c == "remove_snapshot:snap-live"));
        assert!(log.iter().any(|c| c == "remove_snapshot:snap-orphan"));
    }

    #[tokio::test]
    async fn cleanup_with_no_evictions_is_a_noop() {
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        hm.execute(
            make_action(),
            CachingPolicy::Cache { key: "only".into() },
            &NullSink,
            &CancellationToken::new(),
        )
        .await
        .expect("execute should succeed");
        hm.cleanup_deferred_evictions().await;

        let log = calls(&backend);
        assert!(!log.iter().any(|c| c.starts_with("remove_snapshot:")));
    }

    #[tokio::test]
    async fn cache_miss_from_snapshot_passes_workspace() {
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let mut action = make_action();
        // Simulate child step: source is a snapshot, not an image.
        action.source = ImageSource::Snapshot(SnapshotId::new("parent-snap"));

        let result = hm
            .execute(
                action,
                CachingPolicy::Cache {
                    key: "child-step".into(),
                },
                &NullSink,
                &CancellationToken::new(),
            )
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 0);
        assert!(!result.cached);

        let log = calls(&backend);
        // Must restore from snapshot (not create from image).
        assert!(log.iter().any(|c| c.starts_with("restore:parent-snap")));
        assert!(log.iter().any(|c| c.starts_with("exec:")));
        assert!(log.iter().any(|c| c.starts_with("snapshot:")));
    }

    #[tokio::test]
    async fn cancellation_mid_exec_destroys_vm_before_returning() {
        // Cancellation must be cooperative: execute() resolves only after
        // the VM has been destroyed (so the bind-mount ownership reclaim
        // has run) and reports exit 130 with nothing snapshotted or cached.
        let mut backend = MockBackend::new(0, false);
        backend.exec_delay = Some(std::time::Duration::from_secs(30));
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            trigger.cancel();
        });

        let result = hm
            .execute(
                make_action(),
                CachingPolicy::Cache {
                    key: "step-1".into(),
                },
                &NullSink,
                &cancel,
            )
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 130);
        assert!(!result.cached);
        assert!(result.snapshot.is_none());
        assert!(!result.ephemeral_snapshot);

        let log = calls(&backend);
        // The exec started, was abandoned, and the VM was still destroyed
        // before execute() returned.
        assert!(log.iter().any(|c| c.starts_with("exec:")));
        assert!(log.iter().any(|c| c == "destroy"));
        assert!(!log.iter().any(|c| c.starts_with("snapshot:")));
        assert!(hm.registry.is_empty());
    }

    #[tokio::test]
    async fn pre_cancelled_token_skips_vm_creation() {
        let backend = MockBackend::new(0, false);
        let (registry, _dir) = open_temp_registry(10);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let cancel = CancellationToken::new();
        cancel.cancel();

        let result = hm
            .execute(
                make_action(),
                CachingPolicy::Cache {
                    key: "step-1".into(),
                },
                &NullSink,
                &cancel,
            )
            .await
            .expect("execute should succeed");

        assert_eq!(result.exit_code, 130);
        // The biased select bails before any container is created.
        let log = calls(&backend);
        assert!(!log.iter().any(|c| c.starts_with("create:")));
        assert!(!log.iter().any(|c| c.starts_with("exec:")));
    }

    #[tokio::test]
    async fn gc_keeps_tags_queued_for_deferred_eviction() {
        // An LRU-evicted row is gone from SQLite immediately, but in-flight
        // steps of this run may still restore from the evicted tag until
        // the DAG drains. A GC sweep running concurrently with the run must
        // therefore keep any tag in the deferred-eviction queue even though
        // no registry row references it any more.
        let mut backend = MockBackend::new(0, false);
        backend.gc_candidates = vec!["snap-step-a".into(), "snap-orphan".into()];
        let (registry, _dir) = open_temp_registry(1);
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        // Capacity 1: caching "step-b" evicts "step-a"'s row and queues
        // "snap-step-a" for deferred removal.
        for key in ["step-a", "step-b"] {
            hm.execute(
                make_action(),
                CachingPolicy::Cache { key: key.into() },
                &NullSink,
                &CancellationToken::new(),
            )
            .await
            .expect("execute should succeed");
        }
        assert!(hm.registry.get("step-a").is_none());

        let removed = hm
            .gc_orphaned_snapshots("harmont-cache/*", std::time::Duration::from_secs(0))
            .await
            .expect("gc should succeed");

        // Only the true orphan goes; the deferred-eviction tag survives.
        assert_eq!(removed, 1);
        let log = calls(&backend);
        assert!(log.iter().any(|c| c == "gc_remove:snap-orphan"));
        assert!(!log.iter().any(|c| c == "gc_remove:snap-step-a"));

        // End-of-run cleanup still removes it once the DAG has drained.
        hm.cleanup_deferred_evictions().await;
        let log = calls(&backend);
        assert!(log.iter().any(|c| c == "remove_snapshot:snap-step-a"));
    }

    #[tokio::test]
    async fn gc_orphans_removes_unregistered_and_keeps_registered() {
        // The registry is the source of truth: an aged tag with a live row
        // must survive the sweep; an aged tag without one is an orphan and
        // goes.
        let mut backend = MockBackend::new(0, false);
        backend.gc_candidates = vec![
            "harmont-cache/live:aaaabbbbccccdddd".into(),
            "harmont-cache/orphan:eeeeffff00001111".into(),
        ];
        let (registry, _dir) = open_temp_registry(10);
        registry
            .put(
                "harmont-cache/live:aaaabbbbccccdddd",
                &SnapshotId::new("harmont-cache/live:aaaabbbbccccdddd"),
            )
            .expect("put");
        let hm = HmVm::new(Arc::new(backend.clone()), registry, VmConfig::default());

        let removed = hm
            .gc_orphaned_snapshots("harmont-cache/*", std::time::Duration::from_secs(0))
            .await
            .expect("gc should succeed");

        assert_eq!(removed, 1);
        let log = calls(&backend);
        assert!(
            log.iter()
                .any(|c| c == "gc_remove:harmont-cache/orphan:eeeeffff00001111")
        );
        assert!(
            !log.iter()
                .any(|c| c == "gc_remove:harmont-cache/live:aaaabbbbccccdddd")
        );
    }
}
