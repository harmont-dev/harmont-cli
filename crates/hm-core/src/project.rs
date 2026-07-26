//! A Harmont workspace and its resolved configuration.
//!
//! A project is a directory tree rooted at a directory containing `.hm/`.

use std::path::{Path, PathBuf};

use crate::config::ResolvedProjectConfig;
use crate::config::domain::ConfigLoadingError;
use crate::config::project::ProjectConfig;
use crate::config::user::UserConfig;

/// A workspace root and its resolved configuration.
#[derive(Debug, Clone)]
pub struct ProjectContext {
    path: PathBuf,
    config: ResolvedProjectConfig,
}

impl ProjectContext {
    /// Locate the project containing `start` (walking up to a directory with a
    /// `.hm/`) and resolve its config against the `user` layer.
    ///
    /// Returns `None` when no ancestor contains a `.hm/` directory.
    ///
    /// # Errors
    /// [`ConfigLoadingError`] if the project config file is present but
    /// unreadable or malformed.
    pub async fn discover(
        start: &Path,
        user: Option<&UserConfig>,
    ) -> Result<Option<Self>, ConfigLoadingError> {
        match Self::find_root(start) {
            Some(root) => Ok(Some(Self::at(root, user).await?)),
            None => Ok(None),
        }
    }

    /// Wrap the workspace rooted at `path`, resolving its config against the
    /// `user` layer. A missing project config file resolves to defaults.
    ///
    /// # Errors
    /// [`ConfigLoadingError`] if the project config file is present but
    /// unreadable or malformed.
    pub async fn at(path: PathBuf, user: Option<&UserConfig>) -> Result<Self, ConfigLoadingError> {
        let project = Self::load_config(&Self::config_file(&path)).await?;
        let user = user.cloned().unwrap_or_default();
        let config = ResolvedProjectConfig::from_user_project(&user, &project);
        Ok(Self { path, config })
    }

    /// The workspace root (the directory containing `.hm/`).
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "returns &Path via non-const deref coercion from PathBuf"
    )]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `.hm/` directory path.
    #[must_use]
    pub fn hm_dir(&self) -> PathBuf {
        self.path.join(".hm")
    }

    /// The project config file path (`.hm/config.toml`).
    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        Self::config_file(&self.path)
    }

    /// The resolved configuration for this workspace.
    #[must_use]
    pub const fn config(&self) -> &ResolvedProjectConfig {
        &self.config
    }

    /// The project config file path for a workspace root.
    fn config_file(root: &Path) -> PathBuf {
        root.join(".hm").join("config.toml")
    }

    /// Walk up from `start` to the first directory containing `.hm/`.
    ///
    /// Returns the directory *containing* `.hm/`, or `None` at the filesystem
    /// root.
    fn find_root(start: &Path) -> Option<PathBuf> {
        let mut current = start;
        loop {
            if current.join(".hm").is_dir() {
                return Some(current.to_path_buf());
            }
            current = current.parent()?;
        }
    }

    /// Read a project config, treating a missing file as defaults.
    async fn load_config(path: &Path) -> Result<ProjectConfig, ConfigLoadingError> {
        match tokio::fs::read_to_string(path).await {
            Ok(contents) => Ok(toml::from_str(&contents)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(ProjectConfig::default()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn find_project_root_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let nested = tmp.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        assert_eq!(
            ProjectContext::find_root(&nested),
            Some(tmp.path().to_path_buf())
        );
    }

    #[rstest]
    fn find_project_root_none_when_absent() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(ProjectContext::find_root(tmp.path()), None);
    }

    #[rstest]
    #[tokio::test]
    async fn at_defaults_when_no_config_file() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let ctx = ProjectContext::at(tmp.path().to_path_buf(), None)
            .await
            .unwrap();
        assert_eq!(
            ctx.config().backend,
            crate::config::domain::BackendConfig::Docker
        );
        assert_eq!(ctx.config_path(), tmp.path().join(".hm").join("config.toml"));
    }
}
