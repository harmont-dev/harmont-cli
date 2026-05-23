//! Platform directory resolution.
//!
//! Thin wrappers around the [`dirs`] crate that provide consistent
//! return types. Application-specific paths (e.g. `~/.harmont/`)
//! belong in the consuming crate, not here.

use std::path::PathBuf;

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
}
