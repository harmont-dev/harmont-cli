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

/// What to execute inside a VM.
#[derive(Clone, Debug)]
pub struct Action {
    pub source: ImageSource,
    pub cmd: String,
    pub env: Vec<(String, String)>,
    pub working_dir: String,
    pub timeout: Option<Duration>,
    /// Host directory to copy into `working_dir` before execution.
    /// Skipped on cache hits (snapshot already contains prior state).
    pub inject: Option<PathBuf>,
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
