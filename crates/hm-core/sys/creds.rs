//! Credentials to access the Harmont cloud.
//!
//! The cloud issues a single **user-scoped** bearer: `claim_token` /
//! `redeem_code` return one token, and it lists every organization the user
//! belongs to. So there is exactly one credential to store, not a map.
//!
//! A second API base (localhost, staging) is served by the `HM_API_TOKEN`
//! environment variable, which overrides the stored value on resolve.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::sys;

use super::Sys;

/// Failure loading a credentials file.
#[derive(Debug, Error)]
pub enum LoadingError {
    /// File exists but is owned by someone other than the current user.
    #[error(
        "{user} does not own {path}. In order for this to be safe, {user} must own {path}",
        user = .0,
        path = .1.display()
    )]
    FileNotOwned(String, PathBuf),

    /// File exists but is not mode `0o600`.
    #[error(
        "{path} is not chmod 600. In order for this to be secure, {path} must be chmod 0o600",
        path = .0.display()
    )]
    FileNotExclusivelyOwned(PathBuf),

    /// Credentials file could not be read.
    #[error("failed to read {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    /// Credentials file was not valid TOML / shape.
    #[error("failed to parse {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
}

/// On-disk TOML shape: a plain string (serde).
#[derive(Debug, Default, Serialize, Deserialize)]
struct CredsDto {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    token: Option<String>,
}

impl From<&Creds> for CredsDto {
    fn from(creds: &Creds) -> Self {
        Self {
            token: creds
                .token
                .as_ref()
                .map(|s| s.expose_secret().to_owned()),
        }
    }
}

/// The stored Harmont Cloud bearer token.
///
/// Remembers the path it was loaded from so [`Self::set`] / [`Self::clear`]
/// can flush.
#[derive(Debug, Clone)]
pub struct Creds {
    /// Path used for flush (the credentials file, which may not exist yet).
    path: PathBuf,
    /// The stored bearer, if the user has logged in.
    token: Option<SecretString>,
}

impl Creds {
    /// Load credentials from `path`.
    ///
    /// If the file does not exist, returns empty credentials bound to that
    /// path (so a later [`Self::set`] can create it). If the file exists, it
    /// must pass ownership and mode checks before being parsed.
    ///
    /// # Errors
    ///
    /// Returns [`LoadingError`] when the file is insecure, unreadable, or
    /// malformed.
    pub fn load(path: &Path) -> Result<Self, LoadingError> {
        if !path.exists() {
            return Ok(Self {
                path: path.to_path_buf(),
                token: None,
            });
        }

        Self::validate_secure(path)?;

        let contents = fs::read_to_string(path).map_err(|source| LoadingError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let dto: CredsDto = toml::from_str(&contents).map_err(|source| LoadingError::Parse {
            path: path.to_path_buf(),
            source,
        })?;

        Ok(Self {
            path: path.to_path_buf(),
            token: dto.token.map(SecretString::from),
        })
    }

    /// Resolve the bearer token.
    ///
    /// Priority: a non-empty `HM_API_TOKEN` environment variable, then the
    /// stored value. Returns an owned [`SecretString`] so the env override
    /// does not need a place in the store.
    #[must_use]
    pub fn token(&self) -> Option<SecretString> {
        sys::env::hm_api_token()
            .clone()
            .or_else(|| self.token.clone())
    }

    /// Store the bearer and best-effort flush to disk.
    ///
    /// Flush failures are discarded (best-effort persist).
    pub fn set(&mut self, secret: SecretString) {
        self.token = Some(secret);
        let _ = self.flush();
    }

    /// Drop the stored bearer and best-effort flush.
    ///
    /// Note this only clears the *stored* value; a `HM_API_TOKEN` in the
    /// environment still wins on the next [`Self::token`].
    pub fn clear(&mut self) {
        self.token = None;
        let _ = self.flush();
    }

    /// Write the current state to the credentials path (mode `0o600`).
    ///
    /// Uses [`hm_util::os::fs::blocking::write_atomic_restricted`]; requires a
    /// current tokio runtime.
    fn flush(&self) -> io::Result<()> {
        let dto = CredsDto::from(self);
        let serialized = toml::to_string_pretty(&dto)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        hm_util::os::fs::blocking::write_atomic_restricted(
            &self.path,
            serialized.as_bytes(),
            hm_util::os::fs::FileMode(0o600),
            hm_util::os::fs::DirMode(0o700),
        )
    }

    /// Ensure `path` is owned by the current user and mode `0o600`.
    fn validate_secure(path: &Path) -> Result<(), LoadingError> {
        use std::os::unix::fs::MetadataExt;

        let meta = fs::metadata(path).map_err(|source| LoadingError::Read {
            path: path.to_path_buf(),
            source,
        })?;

        if meta.uid() != Sys::whoami_id() {
            return Err(LoadingError::FileNotOwned(Sys::whoami(), path.to_path_buf()));
        }

        if meta.mode() & 0o777 != 0o600 {
            return Err(LoadingError::FileNotExclusivelyOwned(path.to_path_buf()));
        }

        Ok(())
    }
}
