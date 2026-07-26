//! Core of the `hm` CLI.
//!
//! - [`config`] — user/project config resolution.
//! - [`exec`] — the [`exec::ExecutionBackend`] trait and its backends.
//! - [`app_context`] — process-wide toolchain, directories, and user config.
//! - [`project`] — a workspace and its resolved config.

pub mod app_context;
pub mod config;
pub mod exec;
pub mod project;
