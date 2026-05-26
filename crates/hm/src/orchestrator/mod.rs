//! Local-first build orchestration.
//!
//! The orchestrator owns the per-run state: the event bus that
//! announces `BuildEvent`s, the source-archive store served to
//! step-executor plugins, the cancellation atomic, and the chain
//! scheduler that dispatches each step to a plugin via the plan-1
//! plugin host.

pub mod archive;
pub mod cache;
pub mod docker_client;
pub mod events;
pub mod output_subscriber;
pub mod scheduler;
pub mod signal;
pub mod source;
pub mod workspace;

pub use scheduler::run;
