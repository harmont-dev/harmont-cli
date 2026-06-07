//! Core types for the Harmont VM subsystem.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Where a VM image comes from.
#[derive(Debug, Clone)]
pub enum ImageSource {
    /// An OCI / container image reference (e.g. `ubuntu:24.04`).
    Image(String),
    /// A previously captured snapshot.
    Snapshot(SnapshotId),
}

/// A single command to run inside a VM.
#[derive(Debug, Clone)]
pub struct Action {
    /// The image (or snapshot) to boot from.
    pub source: ImageSource,
    /// Shell command to execute.
    pub cmd: String,
    /// Environment variables injected into the guest.
    pub env: HashMap<String, String>,
    /// Working directory inside the guest.
    pub working_dir: PathBuf,
    /// Maximum wall-clock time before the action is killed.
    pub timeout: Duration,
    /// Files to copy into the guest before execution.
    pub inject: Vec<(PathBuf, PathBuf)>,
}

/// Controls whether the VM subsystem caches execution results.
#[derive(Debug, Clone)]
pub enum CachingPolicy {
    /// Never cache; always re-run.
    None,
    /// Cache the result under the given key.
    Cache {
        /// Opaque cache key (typically a content hash).
        key: String,
    },
}

/// Opaque identifier for a VM snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SnapshotId(pub String);

/// The result of executing an [`Action`].
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Process exit code (0 = success).
    pub exit_code: i32,
    /// Snapshot captured after execution, if any.
    pub snapshot: Option<SnapshotId>,
    /// Whether this result was served from cache.
    pub cached: bool,
}

/// Hardware limits for a VM instance.
#[derive(Debug, Clone)]
pub struct VmConfig {
    /// Number of virtual CPUs.
    pub cpus: u32,
    /// Memory in mebibytes.
    pub memory_mib: u32,
}

/// Receives streaming output from a running action.
#[async_trait::async_trait]
pub trait OutputSink: Send + Sync {
    /// Called for each chunk of stdout data.
    async fn on_stdout(&self, data: &[u8]) -> anyhow::Result<()>;
    /// Called for each chunk of stderr data.
    async fn on_stderr(&self, data: &[u8]) -> anyhow::Result<()>;
}

/// An [`OutputSink`] that discards all output.
#[derive(Debug, Default)]
pub struct NullSink;

#[async_trait::async_trait]
impl OutputSink for NullSink {
    async fn on_stdout(&self, _data: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }

    async fn on_stderr(&self, _data: &[u8]) -> anyhow::Result<()> {
        Ok(())
    }
}
