//! Backend trait for pluggable VM implementations.

use std::fmt;

use anyhow::Result;
use async_trait::async_trait;

use crate::types::{OutputSink, SnapshotId, SnapshotLabel, VmConfig, WorkspaceMount};

/// Factory that creates and manages virtual machines.
#[async_trait]
pub trait VmBackend: Send + Sync + fmt::Debug {
    /// Boot a new VM from the given OCI image reference.
    async fn create(
        &self,
        image: &str,
        config: &VmConfig,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<Box<dyn Vm>>;

    /// Restore a VM from a previously taken snapshot.
    async fn restore(
        &self,
        snapshot: &SnapshotId,
        config: &VmConfig,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<Box<dyn Vm>>;

    /// Check whether a snapshot exists in the backend store.
    async fn snapshot_exists(&self, snapshot: &SnapshotId) -> Result<bool>;

    /// Delete a snapshot from the backend store.
    async fn remove_snapshot(&self, snapshot: &SnapshotId) -> Result<()>;

    /// Best-effort garbage collection of snapshots matching `reference`
    /// (a backend-specific reference filter pattern) whose creation time is
    /// older than `older_than`. Returns the number of snapshots removed.
    ///
    /// `keep` is consulted once per matching snapshot tag; tags for which
    /// it returns `true` are retained. Callers use it to protect snapshots
    /// that are still referenced (e.g. by the snapshot registry) so that GC
    /// only ever removes orphans.
    ///
    /// Backends without a notion of aged snapshot storage may keep the
    /// default no-op implementation.
    ///
    /// # Errors
    ///
    /// Returns an error only when the backend cannot be queried at all;
    /// per-snapshot removal failures are logged and skipped.
    async fn gc_snapshots(
        &self,
        reference: &str,
        older_than: std::time::Duration,
        keep: &(dyn for<'a> Fn(&'a str) -> bool + Send + Sync),
    ) -> Result<u64> {
        let _ = (reference, older_than, keep);
        Ok(0)
    }
}

/// Handle to a running virtual machine.
#[async_trait]
pub trait Vm: Send {
    /// Run a command inside the VM and stream output to `sink`.
    async fn exec(
        &self,
        cmd: &str,
        env: &[(String, String)],
        working_dir: &str,
        sink: &dyn OutputSink,
    ) -> Result<i32>;

    /// Capture the current VM state as a named snapshot.
    async fn snapshot(&mut self, label: &SnapshotLabel) -> Result<SnapshotId>;

    /// Tear down the VM and release all resources.
    async fn destroy(&mut self) -> Result<()>;
}
