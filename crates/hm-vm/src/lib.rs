//! Harmont VM subsystem -- lightweight virtual-machine orchestration for
//! hermetic build and test actions.

pub mod backend;
pub mod registry;
pub mod types;
pub mod vm;

#[cfg(feature = "docker-backend")]
pub mod docker;

pub use backend::{Vm, VmBackend};
pub use registry::ImageRegistry;
pub use types::{
    Action, CachingPolicy, ExecutionResult, ImageSource, NullSink, OutputSink, SnapshotId, VmConfig,
};
pub use vm::HmVm;
