//! Wire-level types shared between `hm` crate internals.
//!
//! This crate is pure data: serde structs and enums with no runtime.

#![forbid(unsafe_code)]
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]

pub mod events;
pub mod executor;
pub mod ir;

pub use events::{BuildEvent, PlanSummary, StdStream};
pub use executor::{ArchiveId, ArtifactRef, CacheDecision, ExecutorInput, SnapshotRef, StepResult};
pub use ir::{Cache, Step, StepAction};
