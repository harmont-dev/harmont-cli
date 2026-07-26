//! Configuration scoped to the user (`~/.hm/config.toml`).

use std::path::Path;

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
}
