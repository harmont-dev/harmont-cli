//! File-backed credential store at `~/.config/hm/credentials.toml`.
//!
//! The file is written with [`Privacy::Private`] (0o600, parent dir 0o700)
//! via [`hm_common::fs::write_atomic`], keyed by `(service, account)`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Default, Serialize, Deserialize)]
struct CredentialFile {
    #[serde(default)]
    entries: BTreeMap<String, BTreeMap<String, String>>,
}

fn path() -> Result<PathBuf> {
    let dirs = hm_common::dir_provider::DirProvider::new().context("could not determine config directory")?;
    Ok(dirs.config().join("hm").join("credentials.toml"))
}

fn load() -> CredentialFile {
    let Ok(p) = path() else {
        return CredentialFile::default();
    };
    let Ok(contents) = std::fs::read_to_string(&p) else {
        return CredentialFile::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}

async fn save(file: &CredentialFile) -> Result<()> {
    let p = path()?;
    let serialized = toml::to_string_pretty(file).context("serializing credentials")?;
    hm_common::fs::write_atomic(&p, serialized.as_bytes(), hm_common::fs::Privacy::Private)
        .await
        .with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

/// Read a credential for `(service, account)`. Returns `None` when the
/// file is missing, unreadable, or the entry is absent.
#[must_use]
pub fn get(service: &str, account: &str) -> Option<String> {
    load().entries.get(service)?.get(account).cloned()
}

/// Write a credential. Silently no-ops on I/O failure (best-effort).
pub async fn set(service: &str, account: &str, secret: &str) {
    let mut f = load();
    f.entries
        .entry(service.to_string())
        .or_default()
        .insert(account.to_string(), secret.to_string());
    let _ = save(&f).await;
}

/// Credential `service` name for the cloud bearer token (account = API base URL).
pub const CLOUD_SERVICE: &str = "harmont-cloud";

/// Resolve the cloud bearer token for `api_base`.
///
/// Priority: `HM_API_TOKEN` env (non-empty) first, then the stored
/// credential keyed by `(CLOUD_SERVICE, api_base)`. Returns `None` when
/// neither is present, so the caller can produce a clear "not logged in" error.
#[must_use]
pub fn cloud_token(api_base: &str) -> Option<String> {
    if let Ok(t) = std::env::var("HM_API_TOKEN")
        && !t.is_empty()
    {
        return Some(t);
    }
    get(CLOUD_SERVICE, api_base)
}

/// Persist the cloud bearer token for `api_base`.
///
/// Silently no-ops on I/O failure (matches the best-effort semantics of
/// the underlying [`set`] call).
pub async fn set_cloud_token(api_base: &str, token: &str) {
    set(CLOUD_SERVICE, api_base, token).await;
}

/// Remove any stored cloud bearer token for `api_base`.
///
/// Silently no-ops if the entry is absent or the write fails.
pub async fn forget_cloud_token(api_base: &str) {
    delete(CLOUD_SERVICE, api_base).await;
}

/// Remove a credential. Silently no-ops if the entry is absent or the
/// underlying write fails.
pub async fn delete(service: &str, account: &str) {
    let mut f = load();
    let now_empty = f.entries.get_mut(service).is_some_and(|svc| {
        svc.remove(account);
        svc.is_empty()
    });
    if now_empty {
        f.entries.remove(service);
    }
    let _ = save(&f).await;
}

#[cfg(test)]
#[allow(clippy::unwrap_used, unsafe_code)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[tokio::test]
    async fn round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        // SAFETY: tests are single-threaded for env mutation by Cargo.
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }

        assert_eq!(get("svc", "acct"), None);
        set("svc", "acct", "shh").await;
        assert_eq!(get("svc", "acct").as_deref(), Some("shh"));
        delete("svc", "acct").await;
        assert_eq!(get("svc", "acct"), None);

        unsafe {
            if let Some(v) = prev {
                std::env::set_var("HOME", v);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }
}
