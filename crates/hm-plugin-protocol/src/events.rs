//! Build-time events produced by the orchestrator and consumed by renderers.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::executor::SnapshotRef;
use crate::ir::DurationMs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StdStream {
    Stdout,
    Stderr,
}

#[derive(
    Debug, Clone, PartialEq, Eq, Serialize, Deserialize, derive_more::IsVariant,
)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum BuildEvent {
    BuildStart {
        run_id: Uuid,
        plan: PlanSummary,
        started_at: DateTime<Utc>,
    },
    /// Emitted once, early, when the build first has an identity.
    /// `watch_url` is `Some` for cloud builds.
    BuildAccepted {
        build: BuildRef,
        watch_url: Option<String>,
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
        duration_ms: DurationMs,
        snapshot: Option<SnapshotRef>,
    },
    /// Emitted when any step in a chain returns non-zero. Carries the
    /// failing step's identity so renderers can show a precise
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
        duration_ms: DurationMs,
    },
}

/// Stable identity for a build, shared by `BuildAccepted` and `hm_core::exec::BuildOutcome`.
/// Local builds have a `run_id` only; cloud builds also have `number`/`org`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildRef {
    pub run_id: Uuid,
    pub number: Option<i64>,
    pub org: Option<String>,
    pub pipeline: String,
}

/// Compact summary of the resolved IR included in `BuildStart`. Lets
/// renderers print a header without needing the full pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSummary {
    pub step_count: usize,
    pub chain_count: usize,
    pub default_runner: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[allow(clippy::unwrap_used, reason = "test setup and assertions")]
    #[rstest]
    fn build_accepted_round_trips() {
        let ev = BuildEvent::BuildAccepted {
            build: BuildRef {
                run_id: uuid::Uuid::nil(),
                number: Some(42),
                org: Some("acme".into()),
                pipeline: "ci".into(),
            },
            watch_url: Some("https://app.harmont.dev/acme/ci/builds/42".into()),
        };
        let s = serde_json::to_string(&ev).unwrap();
        let back: BuildEvent = serde_json::from_str(&s).unwrap();
        assert!(matches!(back, BuildEvent::BuildAccepted { .. }));
    }
}
