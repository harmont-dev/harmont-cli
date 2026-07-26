//! Resolving the user and project config layers into one effective view.
//!
//! [`UserConfig`] and [`ProjectConfig`] are sparse, all-optional layers loaded
//! from their respective files. [`ResolvedProjectConfig`] is what the rest of
//! the CLI reads: the two layers merged, project over user, with defaults
//! applied.

pub mod creds;
pub mod domain;
pub mod project;
pub mod user;

use domain::{BackendConfig, BackendDomain};
use project::ProjectConfig;
use user::UserConfig;

/// Resolved cloud backend settings, with layer merging and defaults applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCloudConfig {
    /// Base Harmont domain the `api`/`app` hosts derive from.
    pub domain: BackendDomain,
    /// Organization for cloud runs, if set by either layer.
    pub org: Option<String>,
    /// Repository (`owner/repo`) this project builds, if set.
    pub repo: Option<String>,
    /// Pipeline slug to submit to without re-prompting, if set.
    pub default_pipeline: Option<String>,
}

/// The effective configuration for a run: the user and project layers merged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProjectConfig {
    /// The resolved execution backend.
    pub backend: BackendConfig<ResolvedCloudConfig>,
}

impl ResolvedProjectConfig {
    /// Merge the user and project layers into the effective config.
    #[must_use]
    pub fn from_user_project(user: &UserConfig, project: &ProjectConfig) -> Self {
        let user_cloud = match &user.backend {
            Some(BackendConfig::Cloud(cloud)) => Some(cloud),
            _ => None,
        };

        let backend = match &project.backend {
            Some(BackendConfig::Docker) => BackendConfig::Docker,
            Some(BackendConfig::Cloud(proj)) => BackendConfig::Cloud(ResolvedCloudConfig {
                domain: proj
                    .domain
                    .clone()
                    .or_else(|| user_cloud.and_then(|u| u.domain.clone()))
                    .unwrap_or_default(),
                org: proj
                    .org
                    .clone()
                    .or_else(|| user_cloud.and_then(|u| u.org.clone())),
                repo: proj.repo.clone(),
                default_pipeline: proj.default_pipeline.clone(),
            }),
            None => user_cloud.map_or(BackendConfig::Docker, |cloud| {
                BackendConfig::Cloud(ResolvedCloudConfig {
                    domain: cloud.domain.clone().unwrap_or_default(),
                    org: cloud.org.clone(),
                    repo: None,
                    default_pipeline: None,
                })
            }),
        };

        Self { backend }
    }
}
