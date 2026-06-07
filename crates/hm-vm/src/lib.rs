//! Harmont VM subsystem -- lightweight virtual-machine orchestration for
//! hermetic build and test actions.

pub mod backend;
pub mod registry;
pub mod types;
pub mod vm;

#[cfg(feature = "boxlite-backend")]
pub mod boxlite;

#[cfg(feature = "docker-backend")]
pub mod docker;

pub use types::{
    Action, CachingPolicy, ExecutionResult, ImageSource, NullSink, OutputSink, SnapshotId,
    VmConfig,
};
