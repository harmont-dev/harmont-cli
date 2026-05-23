//! Wire-level types shared between the `hm` binary and `hm` plugins.
//!
//! This crate is pure data: serde structs, enums, and the
//! [`HM_PLUGIN_API_VERSION`] constant. It has no runtime — no async,
//! no Extism, no Tokio. Bumping `HM_PLUGIN_API_VERSION` is the explicit
//! signal that the wire format changed and plugins must be rebuilt.

#![forbid(unsafe_code)]
// schemars 0.8 pulls older indexmap and wit-bindgen via its transitive tree.
// We can't fix that without bumping schemars itself; allow at crate scope so
// the noisy cargo-group lints don't drown out real issues.
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]

pub mod error;
pub mod events;
pub mod executor;
pub mod hook;
pub mod host_abi;
pub mod ir;
pub mod manifest;
pub mod subcommand;

pub use error::{ExitInfo, PluginError};
pub use events::{BuildEvent, PlanSummary, StdStream};
pub use executor::{ArchiveId, ArtifactRef, CacheDecision, ExecutorInput, SnapshotRef, StepResult};
pub use hook::{HookEvent, HookEventKind, HookOutcome, HookPhase};
pub use host_abi::{
    ArchiveReadArgs, CallbackData, DockerCommitArgs, DockerExecArgs, DockerExtractArgs,
    DockerStartArgs, KeyringArgs, KeyringSetArgs, KvScope, Level, LoopbackHandle, LoopbackRecvArgs,
    SocketHandle, SocketReadArgs, SocketWriteArgs, TtyConfirmArgs, TtyPromptArgs,
};
pub use ir::{Cache, CommandStep};
pub use manifest::{
    Capability, ClapJson, JsonSchema, LifecycleHookSpec, OutputFormatterSpec, PluginManifest,
    StepExecutorSpec, SubcommandSpec,
};
pub use subcommand::SubcommandInput;

/// Wire-format version. Plugins whose manifest reports a different
/// version are rejected at load time. Bump when adding *any* new
/// required field to any wire-level struct.
pub const HM_PLUGIN_API_VERSION: u32 = 1;
