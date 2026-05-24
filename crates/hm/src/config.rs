use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const DEFAULT_API_URL: &str = "https://api.harmont.dev";

/// Resolve the Harmont config dir (`~/.harmont/`).
///
/// # Errors
///
/// Returns an error if the user's home directory cannot be determined
/// (the `dirs` crate's platform-specific lookup fails — typically only
/// happens in restrictive sandboxes with no `HOME` / passwd entry).
pub fn user_config_dir() -> Result<PathBuf> {
    hm_util::dirs::harmont_config_dir().context("could not determine home directory")
}

/// User preferences stored alongside the config.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Preferences {
    /// Default output format ("human" or "json").
    pub format: Option<String>,
    /// Whether `hm build create` should auto-watch.
    pub auto_watch: Option<bool>,
}

/// Persistent CLI configuration at `~/.harmont/config.toml`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Config {
    /// Base URL for the Harmont API.
    pub api_url: Option<String>,
    /// Currently active organization slug.
    pub org: Option<String>,
    /// User preferences.
    #[serde(default)]
    pub preferences: Preferences,
}

impl Config {
    /// Returns the path to the config file (`~/.harmont/config.toml`).
    ///
    /// # Errors
    ///
    /// Returns an error if [`user_config_dir`] fails (no home directory
    /// available).
    pub fn path() -> Result<PathBuf> {
        Ok(user_config_dir()?.join("config.toml"))
    }

    /// Load configuration from disk, returning defaults if the file does not exist.
    ///
    /// # Errors
    ///
    /// Returns an error if the config path cannot be resolved, the file
    /// exists but cannot be read (permissions, I/O error), or the file
    /// contents are not valid TOML matching the `Config` shape.
    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let config: Self =
            toml::from_str(&contents).with_context(|| format!("parsing {}", path.display()))?;
        Ok(config)
    }

    /// Persist configuration to disk atomically, with the config directory
    /// (`~/.harmont/`) restricted to 0o700 so adjacent credential
    /// files are not exposed.
    ///
    /// # Errors
    ///
    /// Returns an error if the config path cannot be resolved, the
    /// `Config` cannot be serialized to TOML (only happens for
    /// non-string map keys, which `Config` does not have), or the
    /// atomic write fails (out-of-space, permission denied, parent
    /// directory cannot be created).
    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        let serialized = toml::to_string_pretty(self).context("serializing config")?;
        hm_util::os::fs::blocking::write_atomic_restricted(
            &path,
            serialized.as_bytes(),
            0o644,
            0o700,
        )
        .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// Effective API URL (config value or default).
    #[must_use]
    pub fn api_url(&self) -> &str {
        self.api_url.as_deref().unwrap_or(DEFAULT_API_URL)
    }
}
