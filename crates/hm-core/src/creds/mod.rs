//! Storage for the Harmont cloud bearer token, under `~/.hm/creds/`.

use std::path::{Path, PathBuf};

use secrecy::SecretString;

// Credential files are hardened to owner-only via Unix mode bits in
// `CredsProvider::new`. Windows needs an equivalent ACL-based path (restrict the
// DACL to the current user) before this is safe to ship there. Fail the build
// until that lands, rather than silently storing tokens with inherited ACLs.
#[cfg(windows)]
compile_error!(
    "CredsProvider does not yet harden credential permissions on Windows; \
     implement an ACL-based owner-only path in CredsProvider::new first"
);

/// The `HM_API_TOKEN` env var, which overrides the stored token.
const TOKEN_ENV: &str = "HM_API_TOKEN";

/// Failure to open the credentials store.
#[derive(Debug, thiserror::Error)]
#[error("initializing the credentials store at {path}")]
pub struct CredsInitError {
    path: PathBuf,
    #[source]
    source: std::io::Error,
}

/// The active Harmont cloud bearer token, stored under `<hm_dir>/creds/`.
#[derive(Debug, Clone)]
pub struct CredsProvider {
    creds_path: PathBuf,
}

impl CredsProvider {
    /// Open the store under `hm_dir`, creating `creds/` (owner-only) if absent
    /// and tightening it and any existing entries to owner-only perms.
    ///
    /// # Errors
    /// [`CredsInitError`] if the directory cannot be created or its permissions
    /// cannot be secured.
    pub async fn new(hm_dir: &Path) -> Result<Self, CredsInitError> {
        let creds_path = hm_dir.join("creds");
        let err = |source| CredsInitError {
            path: creds_path.clone(),
            source,
        };

        tokio::fs::create_dir_all(&creds_path).await.map_err(err)?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            tokio::fs::set_permissions(&creds_path, std::fs::Permissions::from_mode(0o700))
                .await
                .map_err(err)?;
            let mut entries = tokio::fs::read_dir(&creds_path).await.map_err(err)?;
            while let Some(entry) = entries.next_entry().await.map_err(err)? {
                if entry.path().is_file() {
                    tokio::fs::set_permissions(
                        entry.path(),
                        std::fs::Permissions::from_mode(0o600),
                    )
                    .await
                    .map_err(err)?;
                }
            }
        }

        Ok(Self { creds_path })
    }

    /// The token file path.
    fn token_file(&self) -> PathBuf {
        self.creds_path.join("token")
    }

    /// Persist `token` as the active bearer token.
    ///
    /// Best-effort: a write failure is logged, not returned.
    pub async fn set(&self, token: &str) {
        if let Err(e) = hm_common::fs::write_atomic(
            self.token_file(),
            token.as_bytes(),
            hm_common::fs::Privacy::Private,
        )
        .await
        {
            tracing::warn!(error = %e, "could not persist the cloud token");
        }
    }

    /// The active bearer token: `HM_API_TOKEN` if set and non-empty, otherwise
    /// the stored token. `None` when neither is present.
    pub async fn get(&self) -> Option<SecretString> {
        if let Ok(token) = std::env::var(TOKEN_ENV)
            && !token.is_empty()
        {
            return Some(SecretString::from(token));
        }
        let stored = tokio::fs::read_to_string(self.token_file()).await.ok()?;
        let stored = stored.trim();
        (!stored.is_empty()).then(|| SecretString::from(stored.to_owned()))
    }

    /// Remove the stored token. A missing token is a no-op.
    ///
    /// # Errors
    /// The underlying [`std::io::Error`] if the token file exists but cannot be
    /// removed.
    pub async fn clear(&self) -> std::io::Result<()> {
        match tokio::fs::remove_file(self.token_file()).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;
    use secrecy::ExposeSecret as _;

    #[rstest]
    #[tokio::test]
    async fn set_get_clear_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let creds = CredsProvider::new(tmp.path()).await.unwrap();

        assert!(creds.get().await.is_none());
        creds.set("hunter2").await;
        assert_eq!(creds.get().await.unwrap().expose_secret(), "hunter2");
        creds.clear().await.unwrap();
        assert!(creds.get().await.is_none());
        // Clearing an already-absent token is a no-op, not an error.
        creds.clear().await.unwrap();
    }
}
