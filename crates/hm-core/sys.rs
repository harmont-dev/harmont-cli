//! Global utility for fetching system-relevant information on `hm`.
//!
//! [`Sys`] is the process-level handle: credentials (and later system config)
//! for the invoking user. Project-scoped state lives elsewhere (workspace).
//!
//! We define the system to mean the scope of the current user. This includes the user's
//! configuration directory, etc.

pub mod creds;
pub mod env;

use std::io;
use std::path::PathBuf;

use hm_config::Config;
use hm_util::path::{AbsPath, AbsPathBuf};
use thiserror::Error;

use creds::Creds;

/// Failure loading process-level [`Sys`] state.
#[derive(Debug, Error)]
pub enum LoadingError {
    /// Platform config directory could not be resolved.
    #[error("could not determine config directory")]
    ConfigDirUnavailable,

    /// Failed to create the config directory.
    #[error("failed to create {}: {source}", .path.display())]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Credentials file failed security checks or parse.
    ///
    /// Transparent: `#[from]` already makes this a `#[source]`, so wrapping the
    /// text here too would print the inner message twice.
    #[error(transparent)]
    Creds(#[from] creds::LoadingError),
}

/// Process-level hm state (credentials, …).
#[derive(Debug)]
pub struct Sys {
    /// Absolute path to `~/.config/hm` (user config root).
    hm_dir: AbsPathBuf,

    creds: Creds,
}

impl Sys {
    /// Return the current user name (best-effort).
    ///
    /// Prefers `$USER`, then `$LOGNAME`, then a `uid N` fallback so ownership
    /// error messages always have something printable.
    #[must_use]
    pub fn whoami() -> String {
        std::env::var("USER")
            .or_else(|_| std::env::var("LOGNAME"))
            .unwrap_or_else(|_| format!("uid {}", Self::whoami_id()))
    }

    /// Return the current real user id.
    #[must_use]
    pub fn whoami_id() -> u32 {
        nix::unistd::Uid::current().as_raw()
    }

    /// Absolute path to the `~/.config/hm` directory.
    #[must_use]
    pub fn hm_dir(&self) -> AbsPath<'_> {
        self.hm_dir.as_abs_path()
    }

    /// `~/.config/hm/` — this user's config root.
    ///
    /// This and its siblings are the `hm`-namespacing policy: the platform only
    /// tells us where *configuration* goes ([`hm_util::os::dirs`]); that we put
    /// ours in an `hm/` subdirectory is our decision, and a user-scoped one, so
    /// it lives here.
    ///
    /// Resolution only — no I/O, no [`Self::load`] required. Callers that just
    /// need a path (layering config, clearing the cache) should not have to
    /// create directories and read credentials to get one.
    ///
    /// `None` means the platform has no config directory.
    #[must_use]
    pub fn config_dir() -> Option<AbsPathBuf> {
        hm_util::os::dirs::config_dir().map(|c| c.join("hm"))
    }

    /// `~/.config/hm/config.toml` — this user's config file.
    #[must_use]
    pub fn config_path() -> Option<AbsPathBuf> {
        Self::config_dir().map(|d| d.join("config.toml"))
    }

    /// This user's configuration: `~/.config/hm/config.toml` + `HM_*` env, with
    /// no project layer.
    ///
    /// The user-scope counterpart to [`crate::Workspace::config`], which layers
    /// a project's `.hm/config.toml` on top of this. Use this one when there is
    /// no workspace, or when a project layer would be wrong — `hm cloud org
    /// switch` writes back to the user file, so merging the project layer in
    /// first would persist project-scoped values into `~/.config/hm/config.toml`.
    ///
    /// When the config directory cannot be resolved the file layer is skipped;
    /// defaults and env still apply.
    ///
    /// # Errors
    ///
    /// Returns [`hm_config::LoadError`] if the config file is malformed.
    pub fn config() -> Result<Config, hm_config::LoadError> {
        Config::load_from_paths(Self::config_path().as_deref(), None)
    }

    /// `~/.cache/hm/` — this user's cache root (regenerable).
    #[must_use]
    pub fn cache_dir() -> Option<AbsPathBuf> {
        hm_util::os::dirs::cache_dir().map(|c| c.join("hm"))
    }

    /// `~/.cache/hm/workspaces/` — COW workspace cache root.
    #[must_use]
    pub fn workspace_cache_dir() -> Option<AbsPathBuf> {
        Self::cache_dir().map(|c| c.join("workspaces"))
    }

    /// Load process-level system state (credentials, …).
    ///
    /// Ensures `~/.config/hm` exists, then loads `credentials.toml` from it.
    ///
    /// # Errors
    ///
    /// Returns [`LoadingError`] when the config directory cannot be resolved or
    /// created, or credentials fail to load.
    pub fn load() -> Result<Self, LoadingError> {
        let hm_dir = Self::config_dir().ok_or(LoadingError::ConfigDirUnavailable)?;

        if !hm_dir.exists() {
            std::fs::create_dir_all(hm_dir.as_abs_path().as_path()).map_err(|source| {
                LoadingError::CreateDir {
                    path: hm_dir.clone().into_path_buf(),
                    source,
                }
            })?;
        }

        let creds_path = hm_dir.join("credentials.toml");
        let creds = Creds::load(creds_path.as_abs_path().as_path())?;
        Ok(Self { hm_dir, creds })
    }

    /// Loaded credentials.
    #[must_use]
    pub const fn creds(&self) -> &Creds {
        &self.creds
    }

    /// Mutable access to credentials (for `set` / `remove`).
    pub const fn creds_mut(&mut self) -> &mut Creds {
        &mut self.creds
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_is_hm_under_platform_config() {
        let p = Sys::config_dir().unwrap();
        assert!(p.ends_with("hm"), "expected path ending in 'hm', got {p}");
        let parent = p.as_abs_path().parent().unwrap();
        assert!(
            parent.as_path().ends_with(".config") || parent.as_path().ends_with("AppData/Roaming"),
            "unexpected parent: {parent}"
        );
    }

    #[test]
    fn config_path_is_config_toml_in_config_dir() {
        let path = Sys::config_path().unwrap();
        let dir = Sys::config_dir().unwrap();
        assert_eq!(path.as_abs_path().parent().unwrap(), dir.as_abs_path());
        assert!(path.ends_with("config.toml"), "got {path}");
    }

    #[test]
    fn cache_dir_is_hm_under_platform_cache() {
        let p = Sys::cache_dir().unwrap();
        assert!(p.ends_with("hm"), "expected path ending in 'hm', got {p}");
    }

    #[test]
    fn workspace_cache_dir_is_under_cache_dir() {
        let p = Sys::workspace_cache_dir().unwrap();
        assert!(p.ends_with("hm/workspaces"), "got {p}");
    }
}
