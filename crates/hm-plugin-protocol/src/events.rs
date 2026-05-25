//! Build-time events. Produced by the orchestrator (host) and fanned
//! out to output formatters, lifecycle hooks, and (via the host
//! re-broadcast of `hm_emit_step_log`) any subscriber.

use chrono::{DateTime, Utc};
use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::executor::SnapshotRef;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Serialize,
    Deserialize,
    DeriveJsonSchema,
    derive_more::IsVariant,
)]
#[serde(rename_all = "snake_case")]
pub enum StdStream {
    Stdout,
    Stderr,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema, derive_more::IsVariant,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum BuildEvent {
    BuildStart {
        run_id: Uuid,
        plan: PlanSummary,
        started_at: DateTime<Utc>,
    },
    StepQueued {
        step_id: Uuid,
        key: String,
        chain_idx: usize,
        /// Key of this step's `BuildsIn` parent, if any. Lets renderers
        /// nest progress bars to reflect the pipeline's DAG structure.
        parent_key: Option<String>,
        /// Human-readable name for display. Falls back to a truncated
        /// command when no explicit label was set in the pipeline DSL.
        display_name: String,
    },
    StepStart {
        step_id: Uuid,
        runner: String,
        image: Option<String>,
    },
    StepLog {
        step_id: Uuid,
        stream: StdStream,
        line: String,
        ts: DateTime<Utc>,
    },
    StepCacheHit {
        step_id: Uuid,
        key: String,
        tag: String,
    },
    StepEnd {
        step_id: Uuid,
        exit_code: i32,
        duration_ms: u64,
        snapshot: Option<SnapshotRef>,
    },
    /// Emitted when any step in a chain returns non-zero. Carries the
    /// failing step's identity so output plugins can render a precise
    /// diagnostic. Distinct from `StepEnd` (per-step) and `BuildEnd`
    /// (per-run).
    ChainFailed {
        chain_idx: usize,
        failed_step_id: Uuid,
        failed_step_key: String,
        exit_code: i32,
        message: String,
        ts: DateTime<Utc>,
    },
    BuildEnd {
        exit_code: i32,
        duration_ms: u64,
    },
}

/// Compact summary of the resolved IR included in `BuildStart`. Lets
/// output formatters print a header without needing the full pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct PlanSummary {
    pub step_count: usize,
    pub chain_count: usize,
    pub default_runner: String,
}
