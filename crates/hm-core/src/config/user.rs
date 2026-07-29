//! Configuration scoped to the user (`~/.hm/config.toml`).

use std::path::Path;

use anyhow::Context as _;
use serde::{Deserialize, Serialize};

use super::domain::{BackendConfig, BackendDomain, ConfigLoadingError};

/// Cloud settings in a user config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserCloudConfig {
    /// Base Harmont domain.
    #[serde(default)]
    pub domain: Option<BackendDomain>,
    /// Default organization for cloud runs.
    pub org: Option<String>,
}

/// The execution backend selected in a user config.
pub type UserBackendConfig = BackendConfig<UserCloudConfig>;

/// Configuration stored in `~/.hm/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UserConfig {
    /// Which execution backend to use.
    pub backend: Option<UserBackendConfig>,
}

impl UserConfig {
    /// Load and parse a user config from `path`.
    ///
    /// # Errors
    /// [`ConfigLoadingError`] if the file cannot be read or is not valid config
    /// TOML.
    pub async fn try_load(path: impl AsRef<Path>) -> Result<Self, ConfigLoadingError> {
        let contents = tokio::fs::read_to_string(path).await?;
        Ok(toml::from_str(&contents)?)
    }

    /// Serialize to `path`, creating parent directories as needed.
    ///
    /// # Errors
    /// [`anyhow::Error`] if serialization or the write fails.
    pub async fn save(&self, path: &Path) -> anyhow::Result<()> {
        let serialized = toml::to_string_pretty(self).context("serializing user config")?;
        hm_common::fs::write_create_all(path, serialized)
            .await
            .with_context(|| format!("writing {}", path.display()))
    }
}
