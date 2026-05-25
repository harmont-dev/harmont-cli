//! Worktree-hash, session-id, container / network name formatters.

use std::path::Path;

use anyhow::Result;
use sha1::{Digest, Sha1};

pub const LABEL_WORKTREE: &str = "harmont.worktree";
pub const LABEL_SLUG: &str = "harmont.slug";
pub const LABEL_SESSION: &str = "harmont.session";
pub const LABEL_DRIVER: &str = "harmont.driver";
pub const DRIVER_LOCAL: &str = "local";

/// Stable 10-hex-char identity for a worktree, derived from the
/// canonical absolute path. Used as a Docker container/network name
/// component and as a label value.
#[must_use]
pub fn worktree_hash(path: &Path) -> String {
    let bytes = path.to_string_lossy();
    let mut hasher = Sha1::new();
    hasher.update(bytes.as_bytes());
    let out = hasher.finalize();
    hex::encode(&out[..5])
}

/// 6 hex chars from a cryptographically secure RNG. Each `hm dev up`
/// generates its own; collisions are avoided by checking against
/// running containers on creation (Docker would 409 anyway).
#[must_use]
pub fn fresh_session_id() -> String {
    use rand::Rng;
    use rand::distributions::Alphanumeric;
    let raw: Vec<u8> = rand::thread_rng()
        .sample_iter(Alphanumeric)
        .take(64)
        .collect();
    // Reduce to 6 lowercase hex chars via sha1 of the random sample.
    let mut hasher = Sha1::new();
    hasher.update(&raw);
    let out = hasher.finalize();
    hex::encode(&out[..3])
}

#[must_use]
pub fn container_name(worktree_hash: &str, slug: &str, session: &str) -> String {
    format!("hm-{worktree_hash}-{slug}-{session}")
}

#[must_use]
pub fn network_name(worktree_hash: &str, session: &str) -> String {
    format!("hm-{worktree_hash}-{session}")
}

/// Resolve the worktree root. Falls back to the absolute current
/// working directory when there's no git repo.
///
/// # Errors
///
/// Returns an error if the cwd is unreadable.
pub fn resolve_worktree_root() -> Result<std::path::PathBuf> {
    use std::process::Command;
    let try_git = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output();
    if let Ok(out) = try_git
        && out.status.success()
    {
        let s = String::from_utf8(out.stdout)?.trim().to_string();
        if !s.is_empty() {
            return Ok(std::path::PathBuf::from(s));
        }
    }
    Ok(std::env::current_dir()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_hash_is_stable() {
        let h1 = worktree_hash(Path::new("/home/marko/myrepo"));
        let h2 = worktree_hash(Path::new("/home/marko/myrepo"));
        assert_eq!(h1, h2);
        assert_eq!(h1.len(), 10);
        assert!(h1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn worktree_hash_differs_per_path() {
        let h1 = worktree_hash(Path::new("/home/marko/myrepo"));
        let h2 = worktree_hash(Path::new("/home/marko/myrepo-wt2"));
        assert_ne!(h1, h2);
    }

    #[test]
    fn session_id_is_six_hex_chars() {
        let s = fresh_session_id();
        assert_eq!(s.len(), 6);
        assert!(s.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn container_name_format() {
        assert_eq!(
            container_name("a1b2c3d4e5", "db", "7a2f91"),
            "hm-a1b2c3d4e5-db-7a2f91",
        );
    }

    #[test]
    fn network_name_format() {
        assert_eq!(network_name("a1b2c3d4e5", "7a2f91"), "hm-a1b2c3d4e5-7a2f91",);
    }
}
