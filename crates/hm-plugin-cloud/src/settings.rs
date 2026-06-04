//! Local app config + credentials, stored under `~/.harmont/`.
//!
//! - `config.toml`        — non-secret: api_url, default org/pipeline.
//! - `credentials.toml`   — bearer tokens keyed by api base (mode 0600).
//!
//! Precedence for the API base: `HARMONT_API_URL` env > `config.toml` >
//! production default. Tokens: `HARMONT_API_TOKEN` env > credentials file.

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::{Context, Result};
use harmont_cloud::HarmontClient;
use serde::{Deserialize, Serialize};

const PROD_API_URL: &str = "https://api.harmont.dev";

/// Resolved context shared by every authenticated verb: the API base and the
/// configured defaults from `config.toml`.
#[derive(Debug, Clone)]
pub struct ResolvedCtx {
    /// Effective API base URL.
    pub api: String,
    /// Default organization slug, if set.
    pub default_org: Option<String>,
    /// Default pipeline slug, if set.
    pub default_pipeline: Option<String>,
}

impl ResolvedCtx {
    /// Resolve the active organization slug or fail with a clear fix.
    ///
    /// # Errors
    ///
    /// Returns an error if no default org is configured.
    pub fn org(&self) -> Result<String> {
        self.default_org.clone().ok_or_else(|| {
            anyhow::anyhow!("no active organization\n  fix: `hm cloud org switch <slug>`")
        })
    }
}

/// Build an authenticated SDK client from local config + credentials.
///
/// Fails fast with a clear message when no token is present.
///
/// # Errors
///
/// Returns an error if config can't be loaded or no token is available.
pub fn client() -> Result<(HarmontClient, ResolvedCtx)> {
    migrate_legacy_state();
    let cfg = CloudConfig::load()?;
    let api = cfg.resolved_api_url(std::env::var("HARMONT_API_URL").ok());
    let token = resolve_token(&api)?.ok_or_else(|| {
        anyhow::anyhow!("not logged in — run `hm cloud login` or set HARMONT_API_TOKEN")
    })?;
    let client = HarmontClient::with_base_url(token, &api);
    Ok((
        client,
        ResolvedCtx {
            api,
            default_org: cfg.default_org,
            default_pipeline: cfg.default_pipeline,
        },
    ))
}

/// Build an anonymous SDK client for the login endpoints, returning the
/// resolved API base alongside it.
///
/// # Errors
///
/// Returns an error if config can't be loaded.
pub fn anon_client() -> Result<(HarmontClient, String)> {
    let cfg = CloudConfig::load()?;
    let api = cfg.resolved_api_url(std::env::var("HARMONT_API_URL").ok());
    Ok((HarmontClient::anonymous(&api), api))
}

/// One-time migration of the legacy `cloud-state.json` (which stored the
/// active org) into `config.toml`'s `default_org`. Best-effort: any error is
/// ignored so a corrupt or unreadable legacy file never blocks the CLI.
fn migrate_legacy_state() {
    let Some(dir) = hm_util::dirs::harmont_config_dir() else {
        return;
    };
    let legacy = dir.join("cloud-state.json");
    let Ok(bytes) = std::fs::read(&legacy) else {
        return; // no legacy file — nothing to do
    };
    #[derive(Deserialize)]
    struct LegacyState {
        active_org: Option<String>,
    }
    let active_org = serde_json::from_slice::<LegacyState>(&bytes)
        .ok()
        .and_then(|s| s.active_org);
    if let Some(org) = active_org
        && let Ok(mut cfg) = CloudConfig::load()
        && cfg.default_org.is_none()
    {
        cfg.default_org = Some(org);
        let _ = cfg.save();
    }
    // Remove the legacy file regardless, so this runs at most once.
    let _ = std::fs::remove_file(&legacy);
}

/// Non-secret CLI config (`~/.harmont/config.toml`).
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CloudConfig {
    /// Override the API base URL.
    pub api_url: Option<String>,
    /// Org used when `--org`/positional is omitted.
    pub default_org: Option<String>,
    /// Pipeline used when omitted (and the repo has >1).
    pub default_pipeline: Option<String>,
}

impl CloudConfig {
    /// Returns the path to `~/.harmont/config.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn path() -> Result<PathBuf> {
        Ok(hm_util::dirs::harmont_config_dir()
            .context("could not determine home directory")?
            .join("config.toml"))
    }

    /// Load from disk, returning defaults when the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be resolved, the file exists
    /// but is unreadable, or the TOML is malformed.
    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => toml::from_str(&s).with_context(|| format!("parse {}", p.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("read {}", p.display())),
        }
    }

    /// Persist to disk atomically (config dir mode 0o700, file mode 0o644).
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be resolved, serialization
    /// fails, or the atomic write fails.
    pub fn save(&self) -> Result<()> {
        let p = Self::path()?;
        let s = toml::to_string_pretty(self).context("serializing config")?;
        hm_util::os::fs::blocking::write_atomic_restricted(&p, s.as_bytes(), 0o644, 0o700)
            .with_context(|| format!("write {}", p.display()))
    }

    /// Resolve the effective API base URL.
    ///
    /// Priority: `env` (caller passes `HARMONT_API_URL` if set) >
    /// `config.toml` > production default.
    #[must_use]
    pub fn resolved_api_url(&self, env: Option<String>) -> String {
        env.or_else(|| self.api_url.clone())
            .unwrap_or_else(|| PROD_API_URL.to_string())
    }
}

/// Bearer tokens keyed by API base URL (`~/.harmont/credentials.toml`, 0600).
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Credentials {
    #[serde(default)]
    tokens: BTreeMap<String, String>,
}

impl Credentials {
    /// Returns the path to `~/.harmont/credentials.toml`.
    ///
    /// # Errors
    ///
    /// Returns an error if the home directory cannot be determined.
    pub fn path() -> Result<PathBuf> {
        Ok(hm_util::dirs::harmont_config_dir()
            .context("could not determine home directory")?
            .join("credentials.toml"))
    }

    /// Load from disk, returning defaults when the file does not exist.
    ///
    /// If the file is present but its TOML is malformed, emits a
    /// `tracing::warn!` with the path and parse error and falls back to
    /// empty credentials, so a corrupt cache never hard-blocks commands.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be resolved or the file exists
    /// but cannot be read (permissions, I/O error).
    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => match toml::from_str(&s) {
                Ok(c) => Ok(c),
                Err(e) => {
                    tracing::warn!(path = %p.display(), error = %e,
                        "ignoring unparseable credentials file; treating as empty");
                    Ok(Self::default())
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("read {}", p.display())),
        }
    }

    /// Return the stored token for `api_base`, if any.
    #[must_use]
    pub fn token_for(&self, api_base: &str) -> Option<String> {
        self.tokens.get(api_base).cloned()
    }

    /// Store a token for `api_base`.
    pub fn set_token(&mut self, api_base: &str, token: String) {
        self.tokens.insert(api_base.into(), token);
    }

    /// Remove any stored token for `api_base`.
    pub fn clear_token(&mut self, api_base: &str) {
        self.tokens.remove(api_base);
    }

    /// Persist to disk atomically (config dir mode 0o700, file mode 0o600).
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be resolved, serialization
    /// fails, or the atomic write fails.
    pub fn save(&self) -> Result<()> {
        let p = Self::path()?;
        let s = toml::to_string_pretty(self).context("serializing credentials")?;
        hm_util::os::fs::blocking::write_atomic_restricted(&p, s.as_bytes(), 0o600, 0o700)
            .with_context(|| format!("write {}", p.display()))
    }
}

/// Resolve the bearer token for `api_base`.
///
/// Priority: `HARMONT_API_TOKEN` env > `~/.harmont/credentials.toml`.
///
/// # Errors
///
/// Returns an error if the credentials file exists but cannot be read.
pub fn resolve_token(api_base: &str) -> Result<Option<String>> {
    if let Ok(t) = std::env::var("HARMONT_API_TOKEN")
        && !t.is_empty()
    {
        return Ok(Some(t));
    }
    Ok(Credentials::load()?.token_for(api_base))
}

/// Persist a bearer token for `api_base` to the credentials file.
///
/// # Errors
///
/// Returns an error if the credentials file can't be loaded or written.
pub fn store_token(api_base: &str, token: &str) -> Result<()> {
    let mut creds = Credentials::load()?;
    creds.set_token(api_base, token.to_string());
    creds.save()
}

/// Remove any stored token for `api_base` from the credentials file.
///
/// # Errors
///
/// Returns an error if the credentials file can't be loaded or written.
pub fn forget_token(api_base: &str) -> Result<()> {
    let mut creds = Credentials::load()?;
    creds.clear_token(api_base);
    creds.save()
}

/// Set the active organization in `config.toml`.
///
/// # Errors
///
/// Returns an error if config can't be loaded or written.
pub fn set_default_org(slug: &str) -> Result<()> {
    let mut cfg = CloudConfig::load()?;
    cfg.default_org = Some(slug.to_string());
    cfg.save()
}

/// Map a raw generated-client error into an `anyhow` error with a readable
/// message. The raw `Error<E>` renders the server's error body (status,
/// headers, decoded value) via its `Display` impl, which holds for any
/// `E: Debug` — true of the generated `types::Error` body.
pub fn map_raw<E: std::fmt::Debug>(e: harmont_cloud_raw::Error<E>) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_toml() {
        let c = CloudConfig {
            api_url: Some("http://localhost:4000".into()),
            default_org: Some("acme".into()),
            default_pipeline: Some("ci".into()),
        };
        let s = toml::to_string(&c).unwrap();
        let back: CloudConfig = toml::from_str(&s).unwrap();
        assert_eq!(back.default_org.as_deref(), Some("acme"));
        assert_eq!(back.api_url.as_deref(), Some("http://localhost:4000"));
    }

    #[test]
    fn resolved_base_prefers_env() {
        let c = CloudConfig {
            api_url: Some("http://from-config".into()),
            default_org: None,
            default_pipeline: None,
        };
        assert_eq!(c.resolved_api_url(None), "http://from-config");
        assert_eq!(
            c.resolved_api_url(Some("http://from-env".into())),
            "http://from-env"
        );
    }

    #[test]
    fn resolved_base_defaults_to_prod() {
        let c = CloudConfig {
            api_url: None,
            default_org: None,
            default_pipeline: None,
        };
        assert_eq!(c.resolved_api_url(None), "https://api.harmont.dev");
    }

    #[test]
    fn credentials_round_trip() {
        let mut creds = Credentials::default();
        creds.set_token("https://api.harmont.dev", "hm_secret".into());
        let s = toml::to_string(&creds).unwrap();
        let back: Credentials = toml::from_str(&s).unwrap();
        assert_eq!(
            back.token_for("https://api.harmont.dev").as_deref(),
            Some("hm_secret")
        );
        assert_eq!(back.token_for("http://other").as_deref(), None);
    }
}
