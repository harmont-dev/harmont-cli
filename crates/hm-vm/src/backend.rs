//! Backend trait for pluggable VM implementations.

use std::fmt;
use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;

use crate::types::{OutputSink, SnapshotId, VmConfig};

/// Factory that creates and manages virtual machines.
#[async_trait]
pub trait VmBackend: Send + Sync + fmt::Debug {
    /// Boot a new VM from the given OCI image reference.
    async fn create(&self, image: &str, config: &VmConfig) -> Result<Box<dyn Vm>>;

    /// Restore a VM from a previously taken snapshot.
    async fn restore(&self, snapshot: &SnapshotId, config: &VmConfig) -> Result<Box<dyn Vm>>;

    /// Check whether a snapshot exists in the backend store.
    async fn snapshot_exists(&self, snapshot: &SnapshotId) -> Result<bool>;

    /// Delete a snapshot from the backend store.
    async fn remove_snapshot(&self, snapshot: &SnapshotId) -> Result<()>;
}

/// Handle to a running virtual machine.
#[async_trait]
pub trait Vm: Send {
    /// Copy a host path into the guest filesystem.
    async fn inject(&self, host_path: &Path, guest_path: &str) -> Result<()>;

    /// Run a command inside the VM and stream output to `sink`.
    async fn exec(
        &self,
        cmd: &str,
        env: &[(String, String)],
        working_dir: &str,
        sink: &dyn OutputSink,
    ) -> Result<i32>;

    /// Capture the current VM state as a named snapshot.
    async fn snapshot(&mut self, label: &str) -> Result<SnapshotId>;

    /// Tear down the VM and release all resources.
    async fn destroy(&mut self) -> Result<()>;
}
