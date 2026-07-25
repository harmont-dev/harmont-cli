//! Global utility for managing the workspace.

use std::io;
use std::path::{Path, PathBuf};

use hm_config::Config;
use hm_util::path::{AbsPath, AbsPathBuf};
use thiserror::Error;

use crate::sys::Sys;

/// Failure resolving or loading a directory as a [`Workspace`].
#[derive(Debug, Error)]
pub enum LoadError {
    #[error("{} does not appear to be a valid path", .0.display())]
    InvalidPath(PathBuf),
    #[error("{} is not a harmont workspace. harmont workspaces must have a `.hm/` directory", .0.display())]
    InvalidWorkspace(PathBuf),

    /// The process working directory could not be read while resolving a root.
    #[error("cannot determine current directory")]
    CurrentDir(#[source] io::Error),

    /// Walked to the filesystem root without finding a `.hm/` directory.
    #[error(
        "no harmont workspace found\n  → run from a directory that contains `.hm/`, or initialize one with `hm init`"
    )]
    NotFound,

    #[error(transparent)]
    Config(#[from] hm_config::LoadError),
}

/// Utility for accessing well-known paths inside a workspace.
///
/// Three constructors, one discovery rule:
///
/// - [`Workspace::resolve`] — the one CLI verbs want. Honors a `--dir`-style
///   override, else walks up from the cwd; no workspace is an error.
/// - [`Workspace::find`] — same rule, but no workspace is `Ok(None)`, for
///   callers that also work outside a project (the `hm cloud` verbs).
/// - [`Workspace::load`] — validates one **specific** project root (the
///   directory that contains `.hm/`), with no discovery.
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
    /// Find the workspace a command should operate on, if there is one.
    ///
    /// The single root-resolution rule for every verb that takes a `--dir`:
    ///
    /// - **`dir` given** — that directory is the project root, verbatim. No
    ///   walk-up: the backend runs discovery against cloned repo roots, and
    ///   walking up out of a clone could bind to an unrelated `.hm/` above it.
    ///   A relative path is resolved against the cwd, since a workspace root
    ///   must be absolute.
    /// - **`dir` absent** — walk up from the cwd, so a verb works from any
    ///   subdirectory of the project.
    ///
    /// `Ok(None)` means only "the walk-up reached `/` without finding `.hm/`" —
    /// for callers that legitimately run outside a project, like the `hm cloud`
    /// verbs. An explicit `dir` that is not a workspace is a user error, not an
    /// absence, so it still fails with [`LoadError::InvalidWorkspace`].
    ///
    /// Verbs that require a workspace want [`Self::resolve`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`LoadError`] if the cwd cannot be determined, or the resolved
    /// root fails [`Self::load`].
    pub fn find(dir: Option<&Path>) -> Result<Option<Self>, LoadError> {
        let root = if let Some(d) = dir {
            if let Some(abs) = AbsPath::new(d) {
                abs.to_abs_path_buf()
            } else {
                AbsPathBuf::current_dir()
                    .map_err(LoadError::CurrentDir)?
                    .join(d)
            }
        } else {
            let start = AbsPathBuf::current_dir().map_err(LoadError::CurrentDir)?;
            match Self::find_root(start.as_abs_path()) {
                Some(root) => root,
                None => return Ok(None),
            }
        };
        Self::load(&root).map(Some)
    }

    /// Resolve the workspace a command should operate on, requiring one.
    ///
    /// [`Self::find`]'s rule, with the absence policy every CLI verb wants: no
    /// workspace is [`LoadError::NotFound`], whose message tells the user how to
    /// get one.
    ///
    /// # Errors
    ///
    /// As [`Self::find`], plus [`LoadError::NotFound`] when the walk-up finds no
    /// `.hm/` directory.
    pub fn resolve(dir: Option<&Path>) -> Result<Self, LoadError> {
        Self::find(dir)?.ok_or(LoadError::NotFound)
    }

    /// Walk up from `start` looking for a directory containing `.hm/`, and
    /// return that directory (the project root), or `None` if the filesystem
    /// root is reached without finding one.
    ///
    /// Private: this is [`Self::find`]'s discovery half, and callers that want
    /// to know whether they are in a project should ask for the workspace
    /// itself. Handing out a bare root invites re-deriving the paths and config
    /// layering that [`Self::load`] already owns.
    ///
    /// Takes an [`AbsPath`] because the walk only terminates meaningfully from
    /// an absolute start: a relative `start` would walk to the empty path rather
    /// than `/`, and would yield a root that means something different once the
    /// cwd changes. Absolute in, absolute out.
    fn find_root(start: AbsPath<'_>) -> Option<AbsPathBuf> {
        let mut current = start;
        loop {
            if current.join(".hm").is_dir() {
                return Some(current.to_abs_path_buf());
            }
            current = current.parent()?;
        }
    }

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
        let config = Config::load_from_paths(
            Sys::config_path().as_deref(),
            Some(&hm_dir.join("config.toml")),
        )?;
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn abs(p: &Path) -> AbsPath<'_> {
        AbsPath::new(p).unwrap()
    }

    #[test]
    fn find_root_at_start_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let found = Workspace::find_root(abs(tmp.path()));
        assert_eq!(found, AbsPathBuf::new(tmp.path().to_path_buf()));
    }

    #[test]
    fn find_root_walks_up() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join(".hm")).unwrap();
        let nested = tmp.path().join("src").join("deep");
        std::fs::create_dir_all(&nested).unwrap();
        let found = Workspace::find_root(abs(&nested));
        assert_eq!(found, AbsPathBuf::new(tmp.path().to_path_buf()));
    }

    #[test]
    fn find_root_returns_none_when_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let found = Workspace::find_root(abs(tmp.path()));
        assert_eq!(found, None);
    }
}
