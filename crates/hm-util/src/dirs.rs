//! Harmont config directory discovery.
//!
//! Searches for the harmont config directory by checking, in order:
//! 1. `~/.hm`
//! 2. `/etc/hm` (or `C:\ProgramData\hm` on Windows)
//!
//! The first directory that exists on disk wins. The directory does not
//! need to be well-formed to be selected — existence is sufficient.
//!
//! Windows uses `C:\ProgramData`. macOS uses `~/.hm` rather than
//! `~/Library/Application Support` because that confuses everyone.

use std::io;
use std::path::PathBuf;

/// Find the best harmont config directory.
///
/// Returns the first existing directory from the search order:
/// `~/.hm`, then the system config dir (`/etc/hm`).
///
/// Returns `None` if no candidate directory exists.
pub async fn config_dir() -> Option<PathBuf> {
    let candidates = [
        crate::os::dirs::home_dir().map(|h| h.join(".hm")),
        Some(crate::os::dirs::sys_config_dir().join("hm")),
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
/// Same search as [`config_dir`], but returns an [`io::Error`] if no
/// candidate directory is found.
///
/// # Errors
///
/// Returns [`io::ErrorKind::NotFound`] if neither `~/.hm` nor the
/// system config directory exists.
pub async fn config_dir_required() -> io::Result<PathBuf> {
    config_dir().await.ok_or_else(|| {
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

    #[tokio::test]
    async fn config_dir_does_not_panic() {
        let _ = config_dir().await;
    }

    #[tokio::test]
    async fn config_dir_required_gives_not_found_when_missing() {
        let result = config_dir_required().await;
        if let Err(e) = result {
            assert_eq!(e.kind(), io::ErrorKind::NotFound);
        }
    }
}
