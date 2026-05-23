//! Directory resolution for Harmont.
//!
//! Provides both platform-level primitives (`home_dir`, `config_dir`,
//! `sys_config_dir`) and Harmont-specific config directory discovery
//! (`harmont_config_dir`).
//!
//! This is the **only public directory API** in `hm-util`. The
//! low-level `os::dirs` module is `pub(crate)` and must not be used
//! outside this crate — consumers should use this module instead.

use std::io;
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Platform primitives
// ---------------------------------------------------------------------------

/// Platform home directory (`~/` on Unix, `C:\Users\<user>` on Windows).
#[must_use]
pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

/// Platform user config directory (`~/.config` on Linux,
/// `~/Library/Application Support` on macOS, `%APPDATA%` on Windows).
///
/// Respects `$XDG_CONFIG_HOME` on Linux.
#[must_use]
pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir()
}

/// System-wide config directory (`/etc` on Unix, `C:\ProgramData` on
/// Windows).
#[must_use]
pub fn sys_config_dir() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/etc")
    }

    #[cfg(windows)]
    {
        std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
    }
}

// ---------------------------------------------------------------------------
// Harmont-specific discovery
// ---------------------------------------------------------------------------

/// Find the best harmont config directory.
///
/// Searches for the first existing directory in order:
/// 1. `~/.hm`
/// 2. `/etc/hm` (or `C:\ProgramData\hm` on Windows)
///
/// The directory does not need to be well-formed — existence is
/// sufficient. macOS uses `~/.hm` rather than
/// `~/Library/Application Support` because that confuses everyone.
///
/// Returns `None` if no candidate directory exists.
pub async fn harmont_config_dir() -> Option<PathBuf> {
    let candidates = [
        home_dir().map(|h| h.join(".hm")),
        Some(sys_config_dir().join("hm")),
    ];

    for candidate in candidates.into_iter().flatten() {
        if tokio::fs::try_exists(&candidate).await.unwrap_or(false) {
            return Some(candidate);
        }
    }
    None
}

/// Find the best harmont config directory, or return an error.
///
/// Same search as [`harmont_config_dir`], but returns an [`io::Error`]
/// if no candidate directory is found.
///
/// # Errors
///
/// Returns [`io::ErrorKind::NotFound`] if neither `~/.hm` nor the
/// system config directory exists.
pub async fn harmont_config_dir_required() -> io::Result<PathBuf> {
    harmont_config_dir().await.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "no harmont config directory found (searched ~/.hm, /etc/hm)",
        )
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_resolves() {
        let p = home_dir().unwrap();
        assert!(p.exists(), "home dir should exist: {}", p.display());
    }

    #[test]
    fn config_dir_resolves() {
        let p = config_dir().unwrap();
        assert!(
            p.to_string_lossy().len() > 1,
            "config dir should be a real path"
        );
    }

    #[test]
    fn sys_config_dir_is_etc() {
        let p = sys_config_dir();
        #[cfg(unix)]
        assert_eq!(p, PathBuf::from("/etc"));
    }

    #[tokio::test]
    async fn harmont_config_dir_does_not_panic() {
        let _ = harmont_config_dir().await;
    }

    #[tokio::test]
    async fn harmont_config_dir_required_gives_not_found_when_missing() {
        let result = harmont_config_dir_required().await;
        if let Err(e) = result {
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
        }
    }
}
