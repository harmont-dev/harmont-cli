#![allow(
    clippy::multiple_crate_versions,
    reason = "transitive dependency version conflicts in rand/windows-sys/thiserror chains; not fixable without upstream updates"
)]
// The `dirs` crate must NOT be added as a direct dependency of this
// crate. Directory resolution is split by scope and both halves are
// single sources of truth: `hm_util::os::dirs` owns the `dirs`
// dependency and exposes the raw platform roots, and `hm_core::Sys`
// owns where *this user's* hm state lives under them
// (`~/.config/hm`, `~/.cache/hm`). Adding `dirs` here would bypass both.

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
pub mod context;
pub mod error;
pub(crate) mod signal;
