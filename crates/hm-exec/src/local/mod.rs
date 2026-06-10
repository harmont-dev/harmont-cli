//! Local execution backend.
//!
//! Runs the whole build in-process via a DAG scheduler. Each step is
//! executed inside a lightweight VM by the [`runner::vm::VmRunner`], which
//! drives the [`hm_vm`] subsystem (a [`hm_vm::VmBackend`] + snapshot
//! registry). Caching is owned by `hm-vm`, not the scheduler.
mod archive;
mod backend;
mod cache;
mod events;
pub mod runner;
mod scheduler;
mod source;

pub use backend::LocalBackend;
pub(crate) use runner::RunnerRegistry; // intra-crate: local/backend.rs via crate::local::
pub(crate) use runner::vm::VmRunner; // intra-crate: local/backend.rs via crate::local::
pub(crate) use scheduler::chain_count;
pub(crate) use scheduler::run;
pub(crate) use source::{build_archive_bytes, top_level_sizes}; // intra-crate: cloud/backend.rs via crate::local::
