//! Process-wide application context: resolved toolchain, directories, and the
//! user-scoped config.
//!
//! Built once in `main` via [`AppContext::init`] and threaded through the
//! application by shared reference (typically leaked to `&'static` so it can be
//! held across `await` points).

use std::path::{Path, PathBuf};

use hm_common::dir_provider::DirProvider;
use hm_common::git::Git;
use hm_common::process::{ExecutableNotFound, pathbin};
use hm_common::python::Python;

use crate::config::domain::ConfigLoadingError;
use crate::config::user::UserConfig;

/// Failure to initialize the [`AppContext`].
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// A required executable was missing from `PATH`.
    #[error(transparent)]
    Executables(#[from] ExecutableNotFound),
    /// The current directory could not be read.
    #[error("reading the current directory")]
    Cwd(#[source] std::io::Error),
    /// The Harmont directory layout could not be resolved (no home directory?).
    #[error("could not resolve the Harmont directory layout")]
    Dirs,
    /// The user config file exists but could not be read or parsed.
    #[error("loading the user config")]
    UserConfig(#[source] ConfigLoadingError),
}

/// Resolved toolchain, platform directories, and the user config, captured once
/// at startup.
///
/// Accessed by shared reference; not a global. A missing user config is not an
/// error — [`AppContext::user_config`] returns `None`.
#[derive(Debug)]
pub struct AppContext {
    git: PathBuf,
    python3: PathBuf,
    cwd: PathBuf,
    dirs: DirProvider,
    user_config: Option<UserConfig>,
}

impl AppContext {
    /// Resolve the toolchain, directories, and user config.
    ///
    /// # Errors
    /// [`InitError`] if a required executable is missing from `PATH`, the
    /// current directory cannot be read, the home directory cannot be resolved,
    /// or a present user config file is malformed. A missing user config file
    /// is not an error.
    pub async fn init() -> Result<Self, InitError> {
        let git = pathbin("git")?;
        let python3 = pathbin("python3")?;
        let cwd = std::env::current_dir().map_err(InitError::Cwd)?;
        let dirs = DirProvider::new().ok_or(InitError::Dirs)?;
        let user_config = Self::load_user_config(&Self::user_config_path(&dirs))
            .await
            .map_err(InitError::UserConfig)?;

        Ok(Self {
            git,
            python3,
            cwd,
            dirs,
            user_config,
        })
    }

    /// The user config path (`~/.hm/config.toml`).
    fn user_config_path(dirs: &DirProvider) -> PathBuf {
        dirs.home().join(".hm").join("config.toml")
    }

    /// Read the user config, treating a missing file as [`None`].
    ///
    /// # Errors
    /// [`ConfigLoadingError`] if the file is present but unreadable or malformed.
    async fn load_user_config(path: &Path) -> Result<Option<UserConfig>, ConfigLoadingError> {
        match tokio::fs::read_to_string(path).await {
            Ok(contents) => Ok(Some(toml::from_str(&contents)?)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// The absolute working directory captured at initialization.
    #[must_use]
    #[allow(
        clippy::missing_const_for_fn,
        reason = "returns &Path via non-const deref coercion from PathBuf"
    )]
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    /// The platform user directory roots.
    #[must_use]
    pub const fn dirs(&self) -> &DirProvider {
        &self.dirs
    }

    /// The user config, or `None` when no user config file is present.
    #[must_use]
    pub const fn user_config(&self) -> Option<&UserConfig> {
        self.user_config.as_ref()
    }

    /// The system `git`, bound to a [`Git`] handle.
    #[must_use]
    pub fn git(&self) -> Git<'_> {
        Git::new(&self.git)
    }

    /// The system `python3`, bound to a [`Python`] handle.
    #[must_use]
    pub fn python(&self) -> Python<'_> {
        Python::new(&self.python3)
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
    #[tokio::test]
    async fn init_resolves_absolute_cwd_when_toolchain_present() {
        if pathbin("git").is_err() || pathbin("python3").is_err() {
            eprintln!("skipping: toolchain unavailable");
            return;
        }
        let ctx = AppContext::init().await.unwrap();
        assert!(ctx.cwd().is_absolute());
        // git() binds the resolved git; a fresh temp dir is not a repo.
        let dir = tempfile::tempdir().unwrap();
        assert!(ctx.git().repo(dir.path()).is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn missing_user_config_is_none() {
        let path = Path::new("/nonexistent/harmont-test/.hm/config.toml");
        assert!(AppContext::load_user_config(path).await.unwrap().is_none());
    }

    #[rstest]
    #[tokio::test]
    async fn present_user_config_is_parsed() {
        use std::io::Write as _;
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(b"[backend]\ntype = \"docker\"\n").unwrap();
        let loaded = AppContext::load_user_config(f.path()).await.unwrap();
        assert!(loaded.is_some());
    }

    #[rstest]
    fn init_error_wraps_a_missing_executable() {
        let err: InitError = pathbin("hm-common-no-such-binary-xyz").unwrap_err().into();
        assert!(matches!(err, InitError::Executables(_)));
    }
}
