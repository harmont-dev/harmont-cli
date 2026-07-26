//! Wire types passed to and returned by the step executor.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::ir::CommandStep;

/// Opaque handle to a registered workspace archive. The holder reads the
/// bytes back by id; the contents are never inspected through this handle.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    derive_more::From,
    derive_more::Display,
)]
#[serde(transparent)]
pub struct ArchiveId(pub Uuid);

/// Snapshot reference whose format is chosen by the runner (for the
/// Docker runner, an image tag).
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Hash,
    Serialize,
    Deserialize,
    derive_more::From,
    derive_more::Display,
)]
#[serde(transparent)]
pub struct SnapshotRef(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub key: String,
    pub mime: String,
    pub size_bytes: u64,
}

/// Host-decided cache outcome. The executor honours this; it does
/// not re-decide.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CacheDecision {
    /// Boot from `tag`; skip running `cmd`.
    Hit { tag: SnapshotRef },
    /// Run `cmd`; on success, commit to `tag` and report it back in
    /// `StepResult::committed_snapshot`.
    MissBuildAs { tag: SnapshotRef },
    /// Run `cmd`; do not commit.
    MissNoCommit,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutorInput {
    pub step: CommandStep,
    pub workspace_archive_id: ArchiveId,
    pub env: BTreeMap<String, String>,
    pub workdir: String,
    pub run_id: Uuid,
    pub step_id: Uuid,
    /// Host-decided; see [`CacheDecision`]. Every step has one.
    pub cache_lookup: CacheDecision,

    /// Snapshot tag of the upstream step in this chain (if any),
    /// or of the chain-fork parent. When `Some`, the executor must
    /// boot from this tag rather than `step.image` — that's how
    /// chain-stepwise filesystem inheritance works: the orchestrator
    /// commits a snapshot between steps and the next step boots from
    /// it.
    #[serde(default)]
    pub parent_snapshot: Option<SnapshotRef>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepResult {
    pub exit_code: i32,
    /// `Some(tag)` when the executor wrote a snapshot for this step
    /// (typically only on `CacheDecision::MissBuildAs`).
    pub committed_snapshot: Option<SnapshotRef>,
    pub artifacts: Vec<ArtifactRef>,
}
