//! Harmont-specific directory resolution.
//!
//! Every directory accessor in this module returns a Harmont-namespaced
//! path. Raw platform primitives (`home_dir`, `config_dir`) live in
//! `os::dirs` and are **not** re-exported — callers outside `hm-util`
//! should never need them.

#![allow(clippy::must_use_candidate)]

use std::path::PathBuf;

use crate::os::dirs as platform;

/// `~/.harmont/` — CLI config home (config.toml, credentials.toml).
pub fn harmont_config_dir() -> Option<PathBuf> {
    platform::home_dir().map(|h| h.join(".harmont"))
}

/// `<config_dir>/harmont/` — XDG-aware data root (plugins, state).
pub fn harmont_data_dir() -> Option<PathBuf> {
    platform::config_dir().map(|c| c.join("harmont"))
}

/// `<config_dir>/harmont/plugins/` — user-global plugin directory.
pub fn harmont_plugins_dir() -> Option<PathBuf> {
    harmont_data_dir().map(|d| d.join("plugins"))
}

/// `<config_dir>/harmont/state/` — per-plugin persistent KV state.
pub fn harmont_plugin_state_dir() -> Option<PathBuf> {
    harmont_data_dir().map(|d| d.join("state"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn harmont_config_dir_under_home() {
        let p = harmont_config_dir().unwrap();
        assert!(p.ends_with(".harmont"));
    }

    #[test]
    fn harmont_data_dir_under_config() {
        let p = harmont_data_dir().unwrap();
        assert!(p.ends_with("harmont"));
    }

    #[test]
    fn harmont_plugins_dir_resolves() {
        let p = harmont_plugins_dir().unwrap();
        assert!(p.ends_with("harmont/plugins"));
    }

    #[test]
    fn harmont_plugin_state_dir_resolves() {
        let p = harmont_plugin_state_dir().unwrap();
        assert!(p.ends_with("harmont/state"));
    }
}
