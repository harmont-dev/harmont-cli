//! Core of the `hm` CLI.
//!
//! - [`config`] — user/project config resolution.
//! - [`creds`] — Harmont cloud token storage.
//! - [`exec`] — the [`exec::ExecutionBackend`] trait and its backends.
//! - [`app_ctx`] — process-wide toolchain, directories, and user config.
//! - [`project_ctx`] — a workspace and its resolved config.

pub mod app_ctx;
pub mod config;
pub mod creds;
pub mod env;
pub mod exec;
pub mod project_ctx;
pub mod term;
