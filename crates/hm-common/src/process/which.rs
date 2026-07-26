//! Locating executables on `PATH`.

use std::ffi::OsStr;
use std::path::PathBuf;

/// The requested program was not found on `PATH`.
#[derive(Debug, thiserror::Error)]
#[error("`{program}` was not found on PATH")]
pub struct ExecutableNotFound {
    program: String,
}

/// Resolve `program` to its absolute path on `PATH`.
///
/// For a presence check, call `pathbin(program).is_ok()`.
#[tracing::instrument(skip_all, fields(program = %program.as_ref().to_string_lossy()))]
pub fn pathbin(program: impl AsRef<OsStr>) -> Result<PathBuf, ExecutableNotFound> {
    let program = program.as_ref();
    which::which(program).map_err(|_| ExecutableNotFound {
        program: program.to_string_lossy().into_owned(),
    })
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
}
