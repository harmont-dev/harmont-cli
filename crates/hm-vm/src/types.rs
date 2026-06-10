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

/// Typed instruction for `Vm::snapshot`, describing how the committed
/// snapshot should be tagged.
///
/// This replaces a previously stringly-encoded convention where a bare label
/// meant "ephemeral" and a `repo:tag` label meant "cached", a contract that
/// the producer (`vm.rs`) and consumer (the backend) had to agree on
/// out-of-band. Encoding the distinction as an enum makes it compiler-checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SnapshotLabel {
    /// Uncached snapshot. The backend chooses a unique tag (e.g. the
    /// container id) so concurrent sibling steps do not race to write the
    /// same image reference.
    Ephemeral,
    /// Cached snapshot tagged from this cache key (parsed as `repo:tag`).
    Cached(String),
}

/// Opaque snapshot handle. Backend-specific contents.
///
/// The inner representation is private so a snapshot id can only be minted
/// through [`SnapshotId::new`]; read access goes through the `AsRef<str>` /
/// `Deref<Target = str>` impls or the `Display` impl. This keeps the handle a
/// distinct domain newtype rather than an interchangeable `String`.
#[derive(Clone, Debug, Hash, PartialEq, Eq, derive_more::Display)]
#[display("{_0}")]
pub struct SnapshotId(String);

impl SnapshotId {
    /// Construct a snapshot handle from a backend-specific id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl AsRef<str> for SnapshotId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::ops::Deref for SnapshotId {
    type Target = str;

    fn deref(&self) -> &str {
        &self.0
    }
}

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
