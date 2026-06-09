//! Pluggable CI execution backends. The pluggable boundary is the whole build:
//! [`ExecutionBackend::start`] spawns a build and returns a [`BackendHandle`].
//! (Trait + handle land in a later task.)
#![forbid(unsafe_code)]

mod error;
pub use error::{BackendError, Result};

mod request;
pub use request::{Plan, RunOptions, RunRequest, SourceMeta};

mod outcome;
pub use outcome::{BuildOutcome, BuildStatus, StepResultSummary, StepStatus};

pub use hm_plugin_protocol::events::BuildRef;
