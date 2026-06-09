//! File-backed credential store at `~/.config/hm/credentials.toml`.
//!
//! Replaces the OS keyring as the sole backend. The file is written with
//! mode 0o600 (parent dir 0o700) via [`hm_util::os::fs::blocking::write_atomic_restricted`].
//! Keyed by `(service, account)` to match the host-fn ABI plugins use.

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
    let dir = hm_util::dirs::hm_config_dir()
        .context("could not determine config directory")?;
    Ok(dir.join("credentials.toml"))
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

fn save(file: &CredentialFile) -> Result<()> {
    let p = path()?;
    let serialized = toml::to_string_pretty(file).context("serializing credentials")?;
    hm_util::os::fs::blocking::write_atomic_restricted(&p, serialized.as_bytes(), 0o600, 0o700)
        .with_context(|| format!("writing {}", p.display()))?;
    Ok(())
}

/// Read a credential for `(service, account)`. Returns `None` when the
/// file is missing, unreadable, or the entry is absent.
#[must_use]
pub fn get(service: &str, account: &str) -> Option<String> {
    load().entries.get(service)?.get(account).cloned()
}

/// Write a credential. Silently no-ops on I/O failure so plugin callers
/// match the prior keyring-backed best-effort semantics.
pub fn set(service: &str, account: &str, secret: &str) {
    let mut f = load();
    f.entries
        .entry(service.to_string())
        .or_default()
        .insert(account.to_string(), secret.to_string());
    let _ = save(&f);
}

/// Credential `service` name for the cloud bearer token (account = API base URL).
pub const CLOUD_SERVICE: &str = "harmont-cloud";

/// Resolve the cloud bearer token for `api_base`.
///
/// Priority: `HARMONT_API_TOKEN` env (non-empty) first, then the stored
/// credential keyed by `(CLOUD_SERVICE, api_base)`. Returns `None` when
/// neither is present, so the caller can produce a clear "not logged in" error.
#[must_use]
pub fn cloud_token(api_base: &str) -> Option<String> {
    if let Ok(t) = std::env::var("HARMONT_API_TOKEN")
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
pub fn set_cloud_token(api_base: &str, token: &str) {
    set(CLOUD_SERVICE, api_base, token);
}

/// Remove any stored cloud bearer token for `api_base`.
///
/// Silently no-ops if the entry is absent or the write fails.
pub fn forget_cloud_token(api_base: &str) {
    delete(CLOUD_SERVICE, api_base);
}

/// Remove a credential. Silently no-ops if the entry is absent or the
/// underlying write fails.
pub fn delete(service: &str, account: &str) {
    let mut f = load();
    let now_empty = f.entries.get_mut(service).is_some_and(|svc| {
        svc.remove(account);
        svc.is_empty()
    });
    if now_empty {
        f.entries.remove(service);
    }
    let _ = save(&f);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, unsafe_code)]
mod tests {
    use super::*;

    fn with_home<F: FnOnce()>(f: F) {
        let tmp = tempfile::tempdir().unwrap();
        let prev = std::env::var_os("HOME");
        // SAFETY: tests are single-threaded for env mutation by Cargo.
        unsafe {
            std::env::set_var("HOME", tmp.path());
        }
        f();
        unsafe {
            if let Some(v) = prev {
                std::env::set_var("HOME", v);
            } else {
                std::env::remove_var("HOME");
            }
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn round_trip() {
        with_home(|| {
            assert_eq!(get("svc", "acct"), None);
            set("svc", "acct", "shh");
            assert_eq!(get("svc", "acct").as_deref(), Some("shh"));
            delete("svc", "acct");
            assert_eq!(get("svc", "acct"), None);
        });
    }
}
