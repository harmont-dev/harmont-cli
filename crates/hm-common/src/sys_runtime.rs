//! System runtime context.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::dir_provider::DirProvider;
use crate::git::Git;
use crate::process::{ExecutableNotFound, pathbin};
use crate::python::Python;

/// Failure to initialize the [`SysRuntime`].
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
}

// TODO: This is a process-wide singleton (a global `OnceLock`), which is
// convenient but couples every caller to hidden global state. Consider
// threading an explicit `SysRuntime` handle through the application instead,
// so dependencies are visible in signatures and tests can inject their own.
///
/// Install it with [`init`](Self::init), then read it from anywhere through the
/// associated accessors — no threading required.
#[derive(Debug)]
pub struct SysRuntime {
    git: PathBuf,
    python3: PathBuf,
    cwd: PathBuf,
    dirs: DirProvider,
}

static RUNTIME: OnceLock<SysRuntime> = OnceLock::new();

impl SysRuntime {
    /// Resolve the runtime and install it as the process-wide singleton.
    ///
    /// Call once, early in `main`, before any accessor. A later call is ignored.
    ///
    /// # Errors
    /// [`InitError`] if a required executable is missing from `PATH` or the
    /// current directory cannot be read.
    pub fn init() -> Result<(), InitError> {
        let runtime = Self::resolve()?;
        let _ = RUNTIME.set(runtime);
        Ok(())
    }

    fn resolve() -> Result<Self, InitError> {
        Ok(Self {
            git: pathbin("git")?,
            python3: pathbin("python3")?,
            cwd: std::env::current_dir().map_err(InitError::Cwd)?,
            dirs: DirProvider::new().ok_or(InitError::Dirs)?,
        })
    }

    #[allow(
        clippy::expect_used,
        reason = "accessing the runtime before init is a startup bug, not a runtime error"
    )]
    fn get() -> &'static Self {
        RUNTIME
            .get()
            .expect("SysRuntime::init must be called before the runtime is accessed")
    }

    /// The absolute working directory captured at initialization.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn cwd() -> &'static Path {
        &Self::get().cwd
    }

    /// The platform user directory roots, resolved once at initialization.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn dirs() -> &'static DirProvider {
        &Self::get().dirs
    }

    /// The system `git`, bound to a [`Git`] handle.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn git() -> Git<'static> {
        Git::new(&Self::get().git)
    }

    /// The system `python3`, bound to a [`Python`] handle.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn python() -> Python<'static> {
        Python::new(&Self::get().python3)
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
    fn resolve_reports_toolchain_availability() {
        assert_eq!(
            SysRuntime::resolve().is_ok(),
            pathbin("python3").is_ok() && pathbin("git").is_ok()
        );
    }

    #[rstest]
    fn resolve_captures_an_absolute_cwd() {
        let Ok(runtime) = SysRuntime::resolve() else {
            eprintln!("skipping: toolchain unavailable");
            return;
        };
        assert!(runtime.cwd.is_absolute(), "cwd was {:?}", runtime.cwd);
    }

    #[rstest]
    fn init_installs_a_globally_accessible_runtime() {
        if SysRuntime::resolve().is_err() {
            eprintln!("skipping: toolchain unavailable");
            return;
        }
        SysRuntime::init().unwrap();
        assert!(SysRuntime::cwd().is_absolute());
        // git() binds the resolved git; a fresh temp dir is not a repo.
        let dir = tempfile::tempdir().unwrap();
        assert!(SysRuntime::git().repo(dir.path()).is_err());
    }

    #[rstest]
    fn init_error_wraps_a_missing_executable() {
        let err: InitError = pathbin("hm-common-no-such-binary-xyz").unwrap_err().into();
        assert!(matches!(err, InitError::Executables(_)));
    }
}
