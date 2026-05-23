//! Wire types used as host-function arguments and return values.
//! Plugins import these to talk to the hm host fns; the host imports
//! them to expose those fns.

use std::collections::BTreeMap;

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

use crate::executor::ArchiveId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum KvScope {
    /// Per-plugin, persistent across builds. Stored in
    /// `~/.config/harmont/state/<plugin-name>.kv`.
    Plugin,
    /// Per-build, in memory. Lost when the build ends.
    Build,
    /// Per-step, in memory. Lost when the step ends.
    Step,
}

/// Opaque socket handle returned by `hm_unix_socket_connect`. Bound
/// to the plugin instance that opened it.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveJsonSchema,
    derive_more::From, derive_more::Deref, derive_more::Display,
)]
#[serde(transparent)]
pub struct SocketHandle(pub u64);

/// Opaque handle returned by `hm_spawn_loopback`. Bound to the plugin
/// instance.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, DeriveJsonSchema,
    derive_more::From, derive_more::Deref, derive_more::Display,
)]
#[serde(transparent)]
pub struct LoopbackHandle(pub u64);

/// Host-fn argument struct for the corresponding `hm_archive_read` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveReadArgs {
    pub id: ArchiveId,
    pub offset: u64,
    pub max: u64,
}

/// Host-fn argument struct for the corresponding `hm_loopback_recv` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallbackData {
    pub path: String,
    pub query: BTreeMap<String, String>,
}

/// Host-fn argument struct for the corresponding `hm_keyring_get` / `hm_keyring_delete` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyringArgs {
    pub service: String,
    pub account: String,
}

/// Host-fn argument struct for the corresponding `hm_keyring_set` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyringSetArgs {
    pub service: String,
    pub account: String,
    pub secret: String,
}

/// Host-fn argument struct for the corresponding `hm_loopback_recv` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopbackRecvArgs {
    pub h: LoopbackHandle,
    pub timeout_ms: u32,
}

/// Host-fn argument struct for the corresponding `hm_socket_read` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocketReadArgs {
    pub h: SocketHandle,
    pub max: u64,
}

/// Host-fn argument struct for the corresponding `hm_socket_write` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SocketWriteArgs {
    pub h: SocketHandle,
    pub bytes: Vec<u8>,
}

/// Host-fn argument struct for the corresponding `hm_tty_confirm` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TtyConfirmArgs {
    pub msg: String,
    pub default: bool,
}

/// Host-fn argument struct for the corresponding `hm_tty_prompt` host function.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TtyPromptArgs {
    pub msg: String,
    pub mask: bool,
}

/// Host-fn argument struct for `hm_docker_start_container`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct DockerStartArgs {
    pub image: String,
    pub env: std::collections::BTreeMap<String, String>,
    pub workdir: String,
    pub name_hint: String,
}

/// Host-fn argument struct for `hm_docker_exec`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct DockerExecArgs {
    pub container_id: String,
    pub cmd: Vec<String>,
    pub env: std::collections::BTreeMap<String, String>,
    pub workdir: String,
    /// When `Some`, piped into the exec'd process's stdin (closed after
    /// the write so the process sees EOF). Used for tar-extract.
    pub stdin_archive_id: Option<crate::ArchiveId>,
}

/// Host-fn argument struct for `hm_docker_commit`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct DockerCommitArgs {
    pub container_id: String,
    pub tag: String,
}

/// Host-fn argument struct for `hm_docker_extract_workspace`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct DockerExtractArgs {
    pub container_id: String,
    pub archive_id: crate::ArchiveId,
    pub workdir: String,
}
