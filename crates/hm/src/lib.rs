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
/// Re-export of the shared [`hm_config`] crate under the historical
/// `harmont_cli::config` path so existing consumers and integration tests
/// keep resolving. The layered config + credential store now live in
/// `hm-config` so `hm-plugin-cloud` can share them.
pub use hm_config as config;
/// Re-export the credential store under the historical
/// `harmont_cli::creds_store` path.
pub use hm_config::creds as creds_store;
pub mod context;
pub mod error;
pub(crate) mod signal;
