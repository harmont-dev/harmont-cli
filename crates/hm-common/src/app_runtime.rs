//! Application runtime context.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::git::Git;
use crate::process::{ExecutableNotFound, SystemBins};
use crate::python::Python;

/// Failure to initialize the [`AppRuntime`].
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// A required executable was missing from `PATH`.
    #[error(transparent)]
    Executables(#[from] ExecutableNotFound),
    /// The current directory could not be read.
    #[error("reading the current directory")]
    Cwd(#[source] std::io::Error),
}

/// Process-wide runtime context, resolved once at startup.
///
/// Install it with [`init`](Self::init), then read it from anywhere through the
/// associated accessors — no threading required.
#[derive(Debug)]
pub struct AppRuntime {
    bins: SystemBins,
    cwd: PathBuf,
}

static RUNTIME: OnceLock<AppRuntime> = OnceLock::new();

impl AppRuntime {
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
            bins: SystemBins::resolve()?,
            cwd: std::env::current_dir().map_err(InitError::Cwd)?,
        })
    }

    #[allow(
        clippy::expect_used,
        reason = "accessing the runtime before init is a startup bug, not a runtime error"
    )]
    fn get() -> &'static Self {
        RUNTIME
            .get()
            .expect("AppRuntime::init must be called before the runtime is accessed")
    }

    /// The resolved system executables.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn bins() -> &'static SystemBins {
        &Self::get().bins
    }

    /// The absolute working directory captured at initialization.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn cwd() -> &'static Path {
        &Self::get().cwd
    }

    /// The system `git`, bound to a [`Git`] handle.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn git() -> Git<'static> {
        Git::new(Self::bins().git())
    }

    /// The system `python3`, bound to a [`Python`] handle.
    ///
    /// # Panics
    /// If [`init`](Self::init) has not been called.
    #[must_use]
    pub fn python() -> Python<'static> {
        Python::new(Self::bins().python3())
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
    use crate::process::pathbin;
    use rstest::rstest;

    #[rstest]
    fn resolve_reports_toolchain_availability() {
        assert_eq!(
            AppRuntime::resolve().is_ok(),
            pathbin("python3").is_ok() && pathbin("git").is_ok()
        );
    }

    #[rstest]
    fn resolve_captures_an_absolute_cwd() {
        let Ok(runtime) = AppRuntime::resolve() else {
            eprintln!("skipping: toolchain unavailable");
            return;
        };
        assert!(runtime.cwd.is_absolute(), "cwd was {:?}", runtime.cwd);
    }

    #[rstest]
    fn init_installs_a_globally_accessible_runtime() {
        if AppRuntime::resolve().is_err() {
            eprintln!("skipping: toolchain unavailable");
            return;
        }
        AppRuntime::init().unwrap();
        assert!(AppRuntime::cwd().is_absolute());
        assert!(AppRuntime::bins().git().is_absolute());
        // git() binds the resolved git; a fresh temp dir is not a repo.
        let dir = tempfile::tempdir().unwrap();
        assert!(AppRuntime::git().repo(dir.path()).is_err());
    }

    #[rstest]
    fn init_error_wraps_a_missing_executable() {
        let err: InitError = pathbin("hm-common-no-such-binary-xyz").unwrap_err().into();
        assert!(matches!(err, InitError::Executables(_)));
    }
}
