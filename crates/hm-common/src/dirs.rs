//! Project-directory discovery.
//!
//! The `hm`-namespaced directory *roots* (config, cache, …) live in
//! [`crate::dir_provider`]. This module handles the orthogonal problem of
//! locating the current project by walking up from a starting path.

#![allow(clippy::must_use_candidate)]

use std::path::PathBuf;

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
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn find_project_root_at_current_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let found = find_project_root(tmp.path());
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[rstest]
    fn find_project_root_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let nested = tmp.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let found = find_project_root(&nested);
        assert_eq!(found, Some(tmp.path().to_path_buf()));
    }

    #[rstest]
    fn find_project_root_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let found = find_project_root(tmp.path());
        assert_eq!(found, None);
    }
}
