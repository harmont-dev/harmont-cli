//! Global utility for managing the workspace.

use std::path::{Path, PathBuf};

use hm_config::Config;
use hm_util::path::{AbsPath, AbsPathBuf};
use thiserror::Error;

/// Failure loading a directory as a [`Workspace`].
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("{} does not appear to be a valid path", .0.display())]
    InvalidPath(PathBuf),
    #[error("{} is not a harmont workspace. harmont workspaces must have a `.hm/` directory", .0.display())]
    InvalidWorkspace(PathBuf),
    #[error(transparent)]
    Config(#[from] hm_config::LoadError),
}

/// Utility for accessing well-known paths inside a workspace.
///
/// Construct with [`Workspace::load`] on a **specific** project root (the
/// directory that contains `.hm/`). Callers that need parent-directory
/// discovery should walk first (e.g. [`hm_util::dirs::find_project_root`])
/// and only then call [`Workspace::load`].
#[derive(Debug)]
pub struct Workspace {
    /// Absolute path to the `.hm` directory.
    hm_dir: AbsPathBuf,

    /// The loaded configuration for this workspace.
    ///
    /// Note that this will also pull in the user config as required. In other words, this will
    /// include the `~/.config/hm/config.toml` configuration as well as the overlayed workspace
    /// `.hm/config.toml`.
    config: Config,
}

impl Workspace {
    /// Attempt to load the given directory as a workspace, if it appears to be one.
    ///
    /// We label a workspace any directory which has a `.hm` directory within it.
    /// This does **not** walk parent directories; pass the project root itself.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] when the path is missing/not absolute, has no
    /// `.hm/` directory, or the layered config cannot be loaded.
    pub fn load(workspace_path: &Path) -> Result<Self, LoadError> {
        if !workspace_path.exists() {
            return Err(LoadError::InvalidPath(workspace_path.to_path_buf()));
        }

        let hm_dir = workspace_path.join(".hm");
        if !hm_dir.is_dir() {
            return Err(LoadError::InvalidWorkspace(workspace_path.to_path_buf()));
        }

        let hm_dir = AbsPathBuf::new(hm_dir)
            .ok_or_else(|| LoadError::InvalidPath(workspace_path.to_path_buf()))?;
        let config = Config::load(Some(&hm_dir.join("config.toml")))?;
        Ok(Self { hm_dir, config })
    }

    /// Absolute path to the workspace root (parent of `.hm/`).
    #[must_use]
    pub fn path(&self) -> AbsPath<'_> {
        self.hm_dir
            .as_abs_path()
            .parent()
            .expect("`.hm` always has a parent")
    }

    /// Path to the `.hm` directory.
    #[must_use]
    pub fn hm_dir(&self) -> AbsPath<'_> {
        self.hm_dir.as_abs_path()
    }

    /// Layered configuration for this workspace (user + project + env).
    #[must_use]
    pub const fn config(&self) -> &Config {
        &self.config
    }

    /// Returns the path to the `.env` file at the top level of this workspace.
    #[must_use]
    pub fn env_file_path(&self) -> AbsPathBuf {
        self.path().join(".env")
    }

    /// Path to the `.hm/secrets` secrets file.
    #[must_use]
    pub fn secrets_path(&self) -> AbsPathBuf {
        self.hm_dir().join("secrets")
    }
}
