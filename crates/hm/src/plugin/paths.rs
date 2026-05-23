//! Filesystem locations the plugin host inspects.

// `#[must_use]` would be noise on these three single-line `Option<PathBuf>`
// helpers — the names already describe the only thing the caller can do
// with the return value.
#![allow(clippy::must_use_candidate)]
// The single test asserts the path resolved on this host; if config_dir
// can't produce anything, the test environment is the bug.
#![cfg_attr(test, allow(clippy::expect_used))]

use std::path::PathBuf;

/// `~/.config/harmont/plugins/` (or the platform's XDG equivalent).
/// User-global plugins live here.
pub fn user_plugins_dir() -> Option<PathBuf> {
    hm_util::dirs::harmont_plugins_dir()
}

/// `<cwd>/.harmont/plugins/`. Project-local plugins live here.
pub fn project_plugins_dir() -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .map(|p| p.join(".harmont").join("plugins"))
}

/// Where `hm plugin install` writes plugins.
pub fn install_dir() -> Option<PathBuf> {
    user_plugins_dir()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_plugins_dir_resolves() {
        let p = user_plugins_dir().expect("config dir resolves");
        assert!(p.ends_with("harmont/plugins"));
    }
}
