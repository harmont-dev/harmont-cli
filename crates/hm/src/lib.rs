#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts in rand/windows-sys/thiserror chains; not fixable without upstream updates"
)]
// The `dirs` crate must NOT be added as a direct dependency of this
// crate. Platform-directory resolution goes through
// `hm_common::DirProvider`, which owns the `dirs` dependency. Adding
// `dirs` here would bypass that single source of truth.

#[allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI subcommand handlers are the intended user-facing output sites"
)]
pub mod cli;
pub mod commands;
/// Re-export of [`hm_core::config`] at `harmont_cli::config`.
pub use hm_core::config;
pub mod context;
pub mod error;
pub(crate) mod signal;
