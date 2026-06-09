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

/// `<config_dir>/hm/` — XDG-aware user config root for `config.toml`.
///
/// - Linux/macOS: `~/.config/hm/`
/// - Windows: `{FOLDERID_RoamingAppData}/hm/`
pub fn hm_user_config_dir() -> Option<PathBuf> {
    platform::config_dir().map(|c| c.join("hm"))
}

/// Walk up from `start` looking for a directory containing `.hm/`.
/// Returns the project root (the directory *containing* `.hm/`),
/// or `None` if the filesystem root is reached without finding one.
pub fn find_project_root(start: &std::path::Path) -> Option<PathBuf> {
    let mut current = start;
    loop {
        if current.join(".hm").is_dir() {
            return Some(current.to_path_buf());
        }
        current = current.parent()?;
    }
}

/// `~/.harmont/cache/` — local build cache root.
pub fn harmont_cache_dir() -> Option<PathBuf> {
    harmont_config_dir().map(|h| h.join("cache"))
}

/// `~/.harmont/cache/workspaces/` — COW workspace cache root.
pub fn harmont_workspace_cache_dir() -> Option<PathBuf> {
    harmont_cache_dir().map(|c| c.join("workspaces"))
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

    #[test]
    fn hm_user_config_dir_under_config() {
        let p = hm_user_config_dir().unwrap();
        assert!(p.ends_with("hm"), "expected path ending in 'hm', got {p:?}");
        let parent = p.parent().unwrap();
        assert!(
            parent.ends_with(".config") || parent.ends_with("AppData/Roaming"),
            "unexpected parent: {parent:?}"
        );
    }

    #[test]
    fn find_project_root_at_current_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let found = find_project_root(tmp.path());
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn find_project_root_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let nested = tmp.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let found = find_project_root(&nested);
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[test]
    fn find_project_root_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let found = find_project_root(tmp.path());
        assert_eq!(found, None);
    }

    #[test]
    fn harmont_cache_dir_resolves() {
        let p = harmont_cache_dir().unwrap();
        assert!(p.to_string_lossy().contains("cache"));
    }

    #[test]
    fn harmont_workspace_cache_dir_resolves() {
        let p = harmont_workspace_cache_dir().unwrap();
        assert!(p.to_string_lossy().contains("workspaces"));
    }
}
