//! Harmont-specific directory resolution.
//!
//! Every directory accessor in this module returns an `hm`-namespaced path
//! under an XDG-correct root: configuration in `~/.config/hm/`, regenerable
//! cache in `~/.cache/hm/`. Raw platform primitives (`home_dir`, `config_dir`,
//! `cache_dir`) live in `os::dirs` and are **not** re-exported — callers
//! outside `hm-util` should never need them.

#![allow(clippy::must_use_candidate)]

use std::path::PathBuf;

use crate::os::dirs as platform;

/// `~/.config/hm/` — user config root (`config.toml`, `credentials.toml`).
pub fn hm_config_dir() -> Option<PathBuf> {
    platform::config_dir().map(|c| c.join("hm"))
}

/// `~/.cache/hm/` — local build cache root (regenerable).
pub fn hm_cache_dir() -> Option<PathBuf> {
    platform::cache_dir().map(|c| c.join("hm"))
}

/// `~/.cache/hm/workspaces/` — COW workspace cache root.
pub fn hm_workspace_cache_dir() -> Option<PathBuf> {
    hm_cache_dir().map(|c| c.join("workspaces"))
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn hm_config_dir_under_config() {
        let p = hm_config_dir().unwrap();
        assert!(p.ends_with("hm"), "expected path ending in 'hm', got {p:?}");
        let parent = p.parent().unwrap();
        assert!(
            parent.ends_with(".config") || parent.ends_with("AppData/Roaming"),
            "unexpected parent: {parent:?}"
        );
    }

    #[test]
    fn hm_cache_dir_under_cache() {
        let p = hm_cache_dir().unwrap();
        assert!(p.ends_with("hm"), "expected path ending in 'hm', got {p:?}");
    }

    #[test]
    fn hm_workspace_cache_dir_resolves() {
        let p = hm_workspace_cache_dir().unwrap();
        assert!(p.ends_with("hm/workspaces"), "got {p:?}");
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
}
