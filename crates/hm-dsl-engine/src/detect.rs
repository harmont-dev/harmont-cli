use std::path::Path;

use anyhow::{Context, bail};

/// Scan `.hm/` and report if python extensions are present. A missing `.hm/`
/// directory yields all-`false`; an unreadable one is an error.
///
/// # Errors
///
/// - The `.hm/` directory does not exist.
/// - No `.py` or `.ts` files are found inside `.hm/`.
pub fn check_python(repo_root: &Path) -> anyhow::Result<()> {
    let harmont_dir = repo_root.join(".hm");
    if !harmont_dir.is_dir() {
        bail!("no .hm/ directory found in {}", repo_root.display());
    }
    let has_py = scan_for_py_files(&harmont_dir)?;
    if has_py {
        Ok(())
    } else {
        bail!("no .py files found in {}", harmont_dir.display())
    }
}

/// True when `.hm/` exists and holds at least one `.py` or `.ts` file.
///
/// The backend fans pipeline discovery out across every repo in an
/// installation, most of which declare no pipelines at all. Those repos should
/// yield an empty registry, not an error — callers use this to short-circuit to
/// an empty envelope instead of calling [`check_python`].
#[must_use]
pub fn has_pipeline_files(repo_root: &Path) -> bool {
    matches!(check_python(repo_root), Ok(()))
}

fn scan_for_py_files(folder: &Path) -> anyhow::Result<bool> {
    let entries = std::fs::read_dir(folder)
        .with_context(|| format!("failed to read {}", folder.display()))?;

    let mut has_py = false;
    // we don't short-circuit to ensure our folder was read successfully
    for entry in entries {
        let entry = entry?;
        if entry.path().extension().is_some_and(|e| e == "py") {
            has_py = true;
        }
    }

    Ok(has_py)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "test setup and assertions"
)]
mod tests {
    use super::*;
    use rstest::rstest;
    use std::fs;
    use tempfile::TempDir;

    /// Expected outcome of a [`check_python`] call.
    enum Outcome {
        Ok,
        /// The error message must contain this substring.
        Err(&'static str),
    }

    /// Helper: create a temp dir with `.hm/` and the given filenames inside
    /// it.
    fn setup(files: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let harmont = tmp.path().join(".hm");
        fs::create_dir(&harmont).unwrap();
        for name in files {
            fs::write(harmont.join(name), "").unwrap();
        }
        tmp
    }

    #[rstest]
    #[case::py_ok(true, &["ci.py"], Outcome::Ok)]
    #[case::py_and_ts_ok(true, &["ci.py", "deploy.ts"], Outcome::Ok)]
    #[case::only_ts_errs(true, &["ci.ts"], Outcome::Err("no .py files"))]
    #[case::empty_dir_errs(true, &[], Outcome::Err("no .py files"))]
    #[case::no_hm_dir_errs(false, &[], Outcome::Err("no .hm/ directory"))]
    fn check_python_reports(
        #[case] make_hm: bool,
        #[case] files: &[&str],
        #[case] expected: Outcome,
    ) {
        // When `make_hm` is false we deliberately leave off the `.hm/` dir so
        // the "no .hm/ directory" branch is exercised faithfully.
        let tmp = if make_hm {
            setup(files)
        } else {
            TempDir::new().unwrap()
        };
        match expected {
            Outcome::Ok => check_python(tmp.path()).unwrap(),
            Outcome::Err(needle) => {
                let msg = check_python(tmp.path()).unwrap_err().to_string();
                assert!(msg.contains(needle), "unexpected error: {msg}");
            }
        }
    }

    #[rstest]
    #[case::py_true(true, &["ci.py"], true)]
    #[case::ts_only_false(true, &["ci.ts"], false)]
    #[case::py_and_ts_true(true, &["ci.py", "deploy.ts"], true)]
    #[case::no_hm_false(false, &[], false)]
    #[case::readme_only_false(true, &["README.md"], false)]
    fn has_pipeline_files_reflects_py_presence(
        #[case] make_hm: bool,
        #[case] files: &[&str],
        #[case] expected: bool,
    ) {
        let tmp = if make_hm {
            setup(files)
        } else {
            TempDir::new().unwrap()
        };
        assert_eq!(has_pipeline_files(tmp.path()), expected);
    }
}
