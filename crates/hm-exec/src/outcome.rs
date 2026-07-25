//! The typed result of a backend run.

use chrono::{DateTime, Utc};
use hm_plugin_protocol::events::BuildRef;
use uuid::Uuid;

/// Headline verdict. The CLI projects this to a process exit code; the int is
/// a CLI-local concern, NOT the contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildStatus {
    Passed,
    Failed,
    Canceled,
    TimedOut,
}

impl BuildStatus {
    /// Process exit code for `hm run`. 130 = SIGINT-cancel, 124 = timeout.
    #[must_use]
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::Passed => 0,
            Self::Failed => 1,
            Self::Canceled => 130,
            Self::TimedOut => 124,
        }
    }

    #[must_use]
    pub const fn is_success(self) -> bool {
        matches!(self, Self::Passed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepStatus {
    Passed,
    Failed,
    Skipped,
    Canceled,
    CacheHit,
    TimedOut,
}

#[derive(Debug, Clone)]
pub struct StepResultSummary {
    pub step_id: Uuid,
    pub key: String,
    pub status: StepStatus,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
}

/// The terminal result of a build.
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    pub build: BuildRef,
    pub status: BuildStatus,
    pub steps: Vec<StepResultSummary>,
    pub started_at: DateTime<Utc>,
    pub finished_at: DateTime<Utc>,
    /// `Some` for cloud (dashboard URL); `None` for local.
    pub watch_url: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::passed(BuildStatus::Passed, 0)]
    #[case::failed(BuildStatus::Failed, 1)]
    #[case::canceled(BuildStatus::Canceled, 130)]
    #[case::timed_out(BuildStatus::TimedOut, 124)]
    fn status_maps_to_exit_code(#[case] status: BuildStatus, #[case] code: i32) {
        assert_eq!(status.exit_code(), code);
    }
}
