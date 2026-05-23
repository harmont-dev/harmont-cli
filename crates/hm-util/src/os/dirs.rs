use std::path::PathBuf;

use anyhow::{Context, Result};

/// Platform home directory (`~/` on Unix, `C:\Users\<user>` on Windows).
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined.
pub fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("could not determine home directory")
}

/// Platform config directory (`~/.config` on Linux,
/// `~/Library/Application Support` on macOS, `%APPDATA%` on Windows).
///
/// # Errors
///
/// Returns an error if the config directory cannot be determined.
pub fn config_dir() -> Result<PathBuf> {
    dirs::config_dir().context("could not determine config directory")
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
}
