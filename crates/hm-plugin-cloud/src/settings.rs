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
use serde::{Deserialize, Serialize};

const PROD_API_URL: &str = "https://api.harmont.dev";

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

    /// Load from disk, returning defaults when the file does not exist or
    /// cannot be parsed.
    ///
    /// # Errors
    ///
    /// Returns an error if the path cannot be resolved or the file exists
    /// but cannot be read (permissions, I/O error).
    pub fn load() -> Result<Self> {
        let p = Self::path()?;
        match std::fs::read_to_string(&p) {
            Ok(s) => Ok(toml::from_str(&s).unwrap_or_default()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
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
