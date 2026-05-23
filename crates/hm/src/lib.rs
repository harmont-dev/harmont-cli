#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts in rand/windows-sys/thiserror chains; not fixable without upstream updates"
)]
// The `dirs` crate must NOT be added as a direct dependency of this
// crate. All directory resolution goes through `hm_util::dirs`, which
// owns the `dirs` dependency and provides both platform primitives and
// Harmont-specific discovery. Adding `dirs` here would bypass that
// single source of truth.

#[allow(
    clippy::print_stdout,
    clippy::print_stderr,
    reason = "CLI subcommand handlers are the intended user-facing output sites"
)]
pub mod cli;
pub mod commands;
pub mod config;
pub mod context;
pub mod creds_store;
pub mod error;
pub mod orchestrator;
pub mod output;
pub mod plugin;
