use std::path::PathBuf;
use std::time::Duration;

/// Where to boot the VM from.
#[derive(Clone, Debug)]
pub enum ImageSource {
    /// OCI image reference (e.g., "alpine:latest").
    Image(String),
    /// Fork from a previous snapshot.
    Snapshot(SnapshotId),
}

/// Bind-mount specification for a host workspace directory.
#[derive(Clone, Debug)]
pub struct WorkspaceMount {
    pub host_path: PathBuf,
    pub guest_path: String,
}

/// What to execute inside a VM.
#[derive(Clone, Debug)]
pub struct Action {
    pub source: ImageSource,
    pub cmd: String,
    pub env: Vec<(String, String)>,
    pub working_dir: String,
    pub timeout: Option<Duration>,
    /// Host workspace directory to bind-mount into the VM.
    ///
    /// Bind mounts are EXCLUDED from snapshots (`docker commit` captures
    /// system state only), so workspace contents never persist into the
    /// cache and are never consulted on the cache-hit path.
    pub workspace: Option<WorkspaceMount>,
}

/// How to cache the result.
#[derive(Clone, Debug)]
pub enum CachingPolicy {
    /// Do not cache.
    None,
    /// Cache the resulting snapshot under this key.
    Cache { key: String },
}

/// Opaque snapshot handle. Backend-specific contents.
#[derive(Clone, Debug, Hash, PartialEq, Eq, derive_more::Display)]
#[display("{_0}")]
pub struct SnapshotId(pub String);

/// Result of executing an action.
#[derive(Clone, Debug)]
pub struct ExecutionResult {
    pub exit_code: i32,
    pub snapshot: Option<SnapshotId>,
    pub cached: bool,
    /// True when the snapshot is ephemeral (not registered in the cache)
    /// and must be cleaned up by the caller after downstream steps finish.
    pub ephemeral_snapshot: bool,
}

/// VM resource configuration.
#[derive(Clone, Debug, Default)]
pub struct VmConfig {
    pub cpus: Option<u32>,
    pub memory_mib: Option<u64>,
    pub disk_size_gb: Option<u64>,
}

/// Receives stdout/stderr lines during execution.
pub trait OutputSink: Send + Sync {
    fn on_stdout(&self, line: &str);
    fn on_stderr(&self, line: &str);
}

/// No-op sink for when output is not needed.
#[derive(Debug)]
pub struct NullSink;

impl OutputSink for NullSink {
    fn on_stdout(&self, _line: &str) {}
    fn on_stderr(&self, _line: &str) {}
}
