//! A Harmont workspace and its resolved configuration.
//!
//! A project is a directory tree rooted at a directory containing `.hm/`.

use std::path::{Path, PathBuf};

use crate::app_context::AppContext;
use crate::config::ResolvedProjectConfig;
use crate::config::domain::ConfigLoadingError;
use crate::config::project::ProjectConfig;

/// A workspace root and its config, resolved against an [`AppContext`].
#[derive(Debug, Clone)]
pub struct ProjectContext<'app> {
    app: &'app AppContext,
    path: PathBuf,
    config: ResolvedProjectConfig,
}

impl<'app> ProjectContext<'app> {
    /// Locate the project containing the app's working directory (walking up to
    /// a directory with a `.hm/`) and resolve its config.
    ///
    /// Returns `None` when no ancestor contains a `.hm/` directory.
    ///
    /// # Errors
    /// [`ConfigLoadingError`] if the project config file is present but
    /// unreadable or malformed.
    pub async fn discover(app: &'app AppContext) -> Result<Option<Self>, ConfigLoadingError> {
        match Self::find_root(app.cwd()) {
            Some(root) => Ok(Some(Self::at(app, root).await?)),
            None => Ok(None),
        }
    }

    /// Wrap the workspace rooted at `path`, resolving its config against the
    /// app's user config. A missing project config file resolves to defaults.
    ///
    /// # Errors
    /// [`ConfigLoadingError`] if the project config file is present but
    /// unreadable or malformed.
    pub async fn at(app: &'app AppContext, path: PathBuf) -> Result<Self, ConfigLoadingError> {
        let project = Self::load_config(&Self::config_file(&path)).await?;
        let user = app.user_config().cloned().unwrap_or_default();
        let config = ResolvedProjectConfig::from_user_project(&user, &project);
        Ok(Self { app, path, config })
    }

    /// The application context this workspace resolved against.
    #[must_use]
    pub const fn app(&self) -> &'app AppContext {
        self.app
    }

    /// The workspace root (the directory containing `.hm/`).
    #[must_use]
    pub fn path(&self) -> &Path {
        self.path.as_path()
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
#[allow(
    clippy::unwrap_used,
    clippy::print_stderr,
    reason = "test setup, assertions, and skip diagnostics"
)]
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
    async fn at_exposes_paths_for_a_workspace() {
        let Ok(app) = AppContext::init().await else {
            eprintln!("skipping: toolchain unavailable");
            return;
        };
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();

        let ctx = ProjectContext::at(&app, tmp.path().to_path_buf())
            .await
            .unwrap();
        assert_eq!(ctx.path(), tmp.path());
        assert_eq!(ctx.config_path(), tmp.path().join(".hm").join("config.toml"));
    }
}
