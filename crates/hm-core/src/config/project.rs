//! Configuration scoped to a project (`<project>/.hm/config.toml`).

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::domain::{BackendConfig, BackendDomain, ConfigLoadingError};

/// Cloud settings in a project config: a superset of the user cloud settings,
/// adding project-scoped identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectCloudConfig {
    /// Base Harmont domain.
    pub domain: Option<BackendDomain>,
    /// Organization this project runs under.
    pub org: Option<String>,
    /// Repository (`owner/repo`) this project builds.
    pub repo: Option<String>,
    /// Resolved pipeline slug to submit to without re-prompting.
    pub default_pipeline: Option<String>,
}

/// The execution backend selected in a project config.
pub type ProjectBackendConfig = BackendConfig<ProjectCloudConfig>;

/// Configuration stored in `<project>/.hm/config.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ProjectConfig {
    /// Which execution backend to use.
    pub backend: Option<ProjectBackendConfig>,
}

impl ProjectConfig {
    /// Load and parse a project config from `path`.
    ///
    /// # Errors
    /// [`ConfigLoadingError`] if the file cannot be read or is not valid config
    /// TOML.
    pub async fn try_load(path: impl AsRef<Path>) -> Result<Self, ConfigLoadingError> {
        let contents = tokio::fs::read_to_string(path).await?;
        Ok(toml::from_str(&contents)?)
    }
}
