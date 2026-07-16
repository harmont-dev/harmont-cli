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

    /// Load process-level system state (credentials, …).
    ///
    /// Ensures `~/.config/hm` exists, then loads `credentials.toml` from it.
    ///
    /// # Errors
    ///
    /// Returns [`LoadingError`] when the config directory cannot be resolved or
    /// created, or credentials fail to load.
    pub fn load() -> Result<Self, LoadingError> {
        let hm_dir = hm_util::dirs::hm_config_dir().ok_or(LoadingError::ConfigDirUnavailable)?;

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
