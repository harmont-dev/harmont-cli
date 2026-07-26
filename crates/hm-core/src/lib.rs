//! Core of the `hm` CLI: layered configuration/credentials and the pluggable
//! CI execution backends. Shared by the `hm` binary and `hm-plugin-cloud`.
//!
//! - [`config`] — project/user/env config resolution and credential storage.
//! - [`exec`] — the [`exec::ExecutionBackend`] trait and its local + cloud
//!   implementations.
//! - [`sys_runtime`] — the process-wide system runtime (git/python/dirs/cwd).

pub mod config;
pub mod exec;
pub mod sys_runtime;
