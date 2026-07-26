//! Core of the `hm` CLI: layered configuration, the pluggable CI execution
//! backends, and the application/project runtime context. Shared by the `hm`
//! binary and `hm-plugin-cloud`.
//!
//! - [`config`] — user/project config resolution.
//! - [`exec`] — the [`exec::ExecutionBackend`] trait and its local + cloud
//!   implementations.
//! - [`app_context`] — the process-wide toolchain/directory/user-config context.
//! - [`project`] — a workspace and its resolved config.

pub mod app_context;
pub mod config;
pub mod exec;
pub mod project;
