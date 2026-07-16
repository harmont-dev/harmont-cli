//! Harmont-specific directory resolution.
//!
//! Every directory accessor in this module returns an `hm`-namespaced path
//! under an XDG-correct root: configuration in `~/.config/hm/`, regenerable
//! cache in `~/.cache/hm/`. Raw platform primitives (`home_dir`, `config_dir`,
//! `cache_dir`) live in `os::dirs` and are **not** re-exported — callers
//! outside `hm-util` should never need them.

#![allow(clippy::must_use_candidate)]

use crate::os::dirs as platform;
use crate::path::{AbsPath, AbsPathBuf};

/// `~/.config/hm/` — user config root (`config.toml`, `credentials.toml`).
///
/// `None` means the platform has no config directory. The platform roots
/// resolve from `$HOME`/`%APPDATA%` and so are absolute by construction.
pub fn hm_config_dir() -> Option<AbsPathBuf> {
    platform::config_dir()
        .map(|c| c.join("hm"))
        .and_then(AbsPathBuf::new)
}

/// `~/.cache/hm/` — local build cache root (regenerable).
pub fn hm_cache_dir() -> Option<AbsPathBuf> {
    platform::cache_dir()
        .map(|c| c.join("hm"))
        .and_then(AbsPathBuf::new)
}

/// `~/.cache/hm/workspaces/` — COW workspace cache root.
pub fn hm_workspace_cache_dir() -> Option<AbsPathBuf> {
    hm_cache_dir().map(|c| c.join("workspaces"))
}

/// Walk up from `start` looking for a directory containing `.hm/`.
/// Returns the project root (the directory *containing* `.hm/`),
/// or `None` if the filesystem root is reached without finding one.
///
/// Takes an [`AbsPath`] because the walk only terminates meaningfully from an
/// absolute start: a relative `start` would walk to the empty path rather than
/// `/`, and would yield a relative root that means something different once the
/// cwd changes. Absolute in, absolute out.
pub fn find_project_root(start: AbsPath<'_>) -> Option<AbsPathBuf> {
    let mut current = start;
    loop {
        if current.join(".hm").is_dir() {
            return Some(current.to_abs_path_buf());
        }
        current = current.parent()?;
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use std::path::Path;

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

    fn abs(p: &Path) -> AbsPath<'_> {
        AbsPath::new(p).unwrap()
    }

    #[test]
    fn find_project_root_at_current_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let found = find_project_root(abs(tmp.path()));
        assert_eq!(found, AbsPathBuf::new(tmp.path().to_path_buf()));
    }

    #[test]
    fn find_project_root_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let nested = tmp.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let found = find_project_root(abs(&nested));
        assert_eq!(found, AbsPathBuf::new(tmp.path().to_path_buf()));
    }

    #[test]
    fn find_project_root_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let found = find_project_root(abs(tmp.path()));
        assert_eq!(found, None);
    }
}
