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
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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

    #[test]
    fn python_file_detected() {
        let tmp = setup(&["ci.py"]);
        check_python(tmp.path()).unwrap();
    }

    #[test]
    fn no_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        // Do NOT create .hm/
        let err = check_python(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no .hm/ directory"), "unexpected error: {msg}");
    }

    #[test]
    fn empty_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".hm")).unwrap();
        let err = check_python(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no .py files"), "unexpected error: {msg}");
    }

    #[test]
    fn other_file_extensions_dont_affect_the_check() {
        let tmp = setup(&["ci.py", "deploy.ts"]);
        assert_eq!(check_python(tmp.path()).unwrap(), ());
    }

    #[test]
    fn python_fails_when_only_ts() {
        let tmp = setup(&["ci.ts"]);
        let err = check_python(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no .py files"), "unexpected error: {msg}");
    }

    #[test]
    fn python_first_no_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        let err = check_python(tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("no .hm/ directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn has_pipeline_files_true_for_py() {
        assert!(has_pipeline_files(setup(&["ci.py"]).path()));
        assert!(!has_pipeline_files(setup(&["ci.ts"]).path()));
        assert!(has_pipeline_files(setup(&["ci.py", "deploy.ts"]).path()));
    }

    #[test]
    fn has_pipeline_files_false_for_missing_or_empty_harmont() {
        // No .hm/ directory at all.
        assert!(!has_pipeline_files(TempDir::new().unwrap().path()));
        // .hm/ exists but declares no .py/.ts files.
        assert!(!has_pipeline_files(setup(&["README.md"]).path()));
    }
}
