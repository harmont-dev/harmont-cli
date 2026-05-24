//! Pipeline IR, the v0 wire format consumed by the `hm` binary.
//!
//! The wire format is a petgraph-serde graph. Nodes carry
//! `CommandStep` + resolved env; edges are `EdgeKind` (`BuildsIn` or
//! `DependsOn`). `PipelineGraph` is the top-level type.

#![forbid(unsafe_code)]
#![allow(clippy::multiple_crate_versions, clippy::cargo_common_metadata)]

mod graph;

pub use graph::{Cache, CommandStep, EdgeKind, PipelineGraph, Transition};
