use std::path::Path;

use anyhow::{Context, bail};

use crate::DslLanguage;

/// Detect the DSL language used in a project by scanning `.harmont/` for file
/// extensions. Prefers **TypeScript** when both are present (the `hm run`
/// default).
///
/// # Errors
///
/// - The `.harmont/` directory does not exist.
/// - No `.py` or `.ts` files are found inside `.harmont/`.
pub fn detect_language(repo_root: &Path) -> anyhow::Result<DslLanguage> {
    let harmont_dir = repo_root.join(".harmont");
    if !harmont_dir.is_dir() {
        bail!("no .harmont/ directory found in {}", repo_root.display());
    }
    match scan_extensions(repo_root)? {
        // When both languages are present, prefer TypeScript.
        (_, true) => Ok(DslLanguage::TypeScript),
        (true, false) => Ok(DslLanguage::Python),
        (false, false) => bail!("no .py or .ts files found in {}", harmont_dir.display()),
    }
}

/// Like [`detect_language`] but prefers **Python** when both are present.
///
/// Used by the machine-facing `hm pipelines` / `hm render` commands that the
/// backend shells out to: the Python path is the fully-supported one (the
/// discovery envelope is Python-only today), so a repo carrying both a `.py`
/// and a redundant `.ts` resolves to Python rather than the unsupported TS
/// registry. `hm run` keeps the TypeScript-preferring [`detect_language`].
///
/// # Errors
///
/// - The `.harmont/` directory does not exist.
/// - No `.py` or `.ts` files are found inside `.harmont/`.
pub fn detect_language_python_first(repo_root: &Path) -> anyhow::Result<DslLanguage> {
    let harmont_dir = repo_root.join(".harmont");
    if !harmont_dir.is_dir() {
        bail!("no .harmont/ directory found in {}", repo_root.display());
    }
    match scan_extensions(repo_root)? {
        (true, _) => Ok(DslLanguage::Python),
        (false, true) => Ok(DslLanguage::TypeScript),
        (false, false) => bail!("no .py or .ts files found in {}", harmont_dir.display()),
    }
}

/// True when `.harmont/` exists and holds at least one `.py` or `.ts` file.
///
/// The backend fans pipeline discovery out across every repo in an
/// installation, most of which declare no pipelines at all. Those repos should
/// yield an empty registry, not an error — callers use this to short-circuit to
/// an empty envelope instead of calling [`detect_language_python_first`].
#[must_use]
pub fn has_pipeline_files(repo_root: &Path) -> bool {
    matches!(scan_extensions(repo_root), Ok((py, ts)) if py || ts)
}

/// Scan `.harmont/` and report `(has_py, has_ts)`. A missing `.harmont/`
/// directory yields `(false, false)`; an unreadable one is an error.
fn scan_extensions(repo_root: &Path) -> anyhow::Result<(bool, bool)> {
    let harmont_dir = repo_root.join(".harmont");
    if !harmont_dir.is_dir() {
        return Ok((false, false));
    }

    let entries = std::fs::read_dir(&harmont_dir)
        .with_context(|| format!("failed to read {}", harmont_dir.display()))?;

    let mut has_py = false;
    let mut has_ts = false;
    for entry in entries {
        let entry = entry?;
        match entry.path().extension().and_then(|e| e.to_str()) {
            Some("py") => has_py = true,
            Some("ts") => has_ts = true,
            _ => {}
        }
    }
    Ok((has_py, has_ts))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    /// Helper: create a temp dir with `.harmont/` and the given filenames inside
    /// it.
    fn setup(files: &[&str]) -> TempDir {
        let tmp = TempDir::new().unwrap();
        let harmont = tmp.path().join(".harmont");
        fs::create_dir(&harmont).unwrap();
        for name in files {
            fs::write(harmont.join(name), "").unwrap();
        }
        tmp
    }

    #[test]
    fn python_file_detected() {
        let tmp = setup(&["ci.py"]);
        let lang = detect_language(tmp.path()).unwrap();
        assert_eq!(lang, DslLanguage::Python);
    }

    #[test]
    fn typescript_file_detected() {
        let tmp = setup(&["ci.ts"]);
        let lang = detect_language(tmp.path()).unwrap();
        assert_eq!(lang, DslLanguage::TypeScript);
    }

    #[test]
    fn mixed_languages_prefers_typescript() {
        let tmp = setup(&["ci.py", "deploy.ts"]);
        let lang = detect_language(tmp.path()).unwrap();
        assert_eq!(lang, DslLanguage::TypeScript);
    }

    #[test]
    fn no_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        // Do NOT create .harmont/
        let err = detect_language(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("no .harmont/ directory"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn empty_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".harmont")).unwrap();
        let err = detect_language(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("no .py or .ts files"),
            "unexpected error: {msg}"
        );
    }

    #[test]
    fn python_first_prefers_python_when_mixed() {
        let tmp = setup(&["ci.py", "deploy.ts"]);
        assert_eq!(
            detect_language_python_first(tmp.path()).unwrap(),
            DslLanguage::Python
        );
    }

    #[test]
    fn python_first_falls_back_to_typescript_when_only_ts() {
        let tmp = setup(&["ci.ts"]);
        assert_eq!(
            detect_language_python_first(tmp.path()).unwrap(),
            DslLanguage::TypeScript
        );
    }

    #[test]
    fn python_first_no_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        let err = detect_language_python_first(tmp.path()).unwrap_err();
        assert!(
            err.to_string().contains("no .harmont/ directory"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn has_pipeline_files_true_for_py_and_ts() {
        assert!(has_pipeline_files(setup(&["ci.py"]).path()));
        assert!(has_pipeline_files(setup(&["ci.ts"]).path()));
        assert!(has_pipeline_files(setup(&["ci.py", "deploy.ts"]).path()));
    }

    #[test]
    fn has_pipeline_files_false_for_missing_or_empty_harmont() {
        // No .harmont/ directory at all.
        assert!(!has_pipeline_files(TempDir::new().unwrap().path()));
        // .harmont/ exists but declares no .py/.ts files.
        assert!(!has_pipeline_files(setup(&["README.md"]).path()));
    }
}
