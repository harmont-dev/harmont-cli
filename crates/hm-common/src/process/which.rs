//! Locating executables on `PATH`.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};

/// The requested program was not found on `PATH`.
#[derive(Debug, thiserror::Error)]
#[error("`{program}` was not found on PATH")]
pub struct ExecutableNotFound {
    program: String,
}

/// Resolve `program` to its absolute path on `PATH`.
///
/// For a presence check, call `pathbin(program).is_ok()`.
///
/// # Errors
/// Returns [`ExecutableNotFound`] if `program` is not an executable on `PATH`.
pub fn pathbin(program: impl AsRef<OsStr>) -> Result<PathBuf, ExecutableNotFound> {
    let program = program.as_ref();
    which::which(program).map_err(|_| ExecutableNotFound {
        program: program.to_string_lossy().into_owned(),
    })
}

/// Resolve `git` to its absolute path on `PATH`.
///
/// # Errors
/// [`ExecutableNotFound`] if `git` is not on `PATH`.
pub fn git() -> Result<PathBuf, ExecutableNotFound> {
    pathbin("git")
}

/// Resolve `python3` to its absolute path on `PATH`.
///
/// # Errors
/// [`ExecutableNotFound`] if `python3` is not on `PATH`.
pub fn python3() -> Result<PathBuf, ExecutableNotFound> {
    pathbin("python3")
}

/// The external executables Harmont shells out to, resolved from `PATH`.
#[derive(Debug, Clone)]
pub struct SystemBins {
    python3: PathBuf,
    git: PathBuf,
}

impl SystemBins {
    /// Resolve the toolchain from `PATH`.
    ///
    /// # Errors
    /// [`ExecutableNotFound`] if `python3` or `git` is not on `PATH`.
    pub fn resolve() -> Result<Self, ExecutableNotFound> {
        Ok(Self {
            python3: python3()?,
            git: git()?,
        })
    }

    /// Path to `python3`.
    #[must_use]
    pub fn python3(&self) -> &Path {
        &self.python3
    }

    /// Path to `git`.
    #[must_use]
    pub fn git(&self) -> &Path {
        &self.git
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::present("sh", true)]
    #[case::absent("hm-common-no-such-binary-xyz", false)]
    fn pathbin_reports_resolvability(#[case] program: &str, #[case] resolvable: bool) {
        assert_eq!(pathbin(program).is_ok(), resolvable);
    }

    #[rstest]
    fn pathbin_resolves_to_an_absolute_path() {
        let path = pathbin("sh").unwrap();
        assert!(path.is_absolute(), "expected absolute path, got {path:?}");
        assert_eq!(path.file_name(), Some(OsStr::new("sh")));
    }

    #[rstest]
    fn error_names_the_missing_program() {
        let err = pathbin("hm-common-no-such-binary-xyz").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("hm-common-no-such-binary-xyz"), "program: {msg}");
        assert!(msg.contains("not found on PATH"), "phrasing: {msg}");
    }

    #[rstest]
    #[case::git("git")]
    #[case::python3("python3")]
    fn alias_matches_pathbin(#[case] program: &str) {
        let via_alias = if program == "git" { git() } else { python3() };
        assert_eq!(via_alias.ok(), pathbin(program).ok());
    }

    #[rstest]
    fn resolve_succeeds_when_python3_and_git_present() {
        assert_eq!(SystemBins::resolve().is_ok(), python3().is_ok() && git().is_ok());
    }

    #[rstest]
    fn system_bins_exposes_paths() {
        let bins = SystemBins {
            python3: PathBuf::from("/opt/python3"),
            git: PathBuf::from("/opt/git"),
        };
        assert_eq!(bins.python3(), Path::new("/opt/python3"));
        assert_eq!(bins.git(), Path::new("/opt/git"));
    }
}
