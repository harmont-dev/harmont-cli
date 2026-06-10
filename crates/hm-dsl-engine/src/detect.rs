use std::path::Path;

use anyhow::{Context, bail};

use crate::DslLanguage;

/// Detect the DSL language used in a project by scanning `.hm/` for file
/// extensions.
///
/// A repo carrying **both** a `.py` and a `.ts` pipeline is rejected rather
/// than silently tie-broken: `hm run` (local) and the backend discovery path
/// (`hm pipelines` / `hm render`) both route through this one resolver, so a
/// tie-break here would let the local build run one language while cloud
/// discovery built the other. Failing fast keeps the two in lockstep. Keep a
/// single pipeline language per `.hm/` directory.
///
/// # Errors
///
/// - The `.hm/` directory does not exist.
/// - No `.py` or `.ts` files are found inside `.hm/`.
/// - Both `.py` and `.ts` files are present (ambiguous language).
pub fn detect_language(repo_root: &Path) -> anyhow::Result<DslLanguage> {
    let harmont_dir = repo_root.join(".hm");
    if !harmont_dir.is_dir() {
        bail!("no .hm/ directory found in {}", repo_root.display());
    }
    let langs = scan_extensions(repo_root)?;
    match (langs.has_py, langs.has_ts) {
        (true, true) => bail!(
            "ambiguous pipeline language in {dir}: found both Python (.py) and \
             TypeScript (.ts) pipeline files\n  \
             → keep exactly one pipeline language per .hm/ directory; remove the \
             extra .py or .ts files\n  \
             (otherwise `hm run` and cloud discovery could resolve different \
             languages for the same repo)",
            dir = harmont_dir.display()
        ),
        (false, true) => Ok(DslLanguage::TypeScript),
        (true, false) => Ok(DslLanguage::Python),
        (false, false) => bail!("no .py or .ts files found in {}", harmont_dir.display()),
    }
}

/// True when `.hm/` exists and holds at least one `.py` or `.ts` file.
///
/// The backend fans pipeline discovery out across every repo in an
/// installation, most of which declare no pipelines at all. Those repos should
/// yield an empty registry, not an error — callers use this to short-circuit to
/// an empty envelope instead of calling [`detect_language`].
#[must_use]
pub fn has_pipeline_files(repo_root: &Path) -> bool {
    matches!(scan_extensions(repo_root), Ok(langs) if langs.has_py || langs.has_ts)
}

/// Which DSL extensions a `.hm/` scan turned up. Named fields make a py/ts
/// swap at a call site impossible to express, unlike a bare `(bool, bool)`.
struct DetectedLangs {
    has_py: bool,
    has_ts: bool,
}

/// Scan `.hm/` and report which DSL extensions are present. A missing `.hm/`
/// directory yields all-`false`; an unreadable one is an error.
fn scan_extensions(repo_root: &Path) -> anyhow::Result<DetectedLangs> {
    let harmont_dir = repo_root.join(".hm");
    if !harmont_dir.is_dir() {
        return Ok(DetectedLangs {
            has_py: false,
            has_ts: false,
        });
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
    Ok(DetectedLangs { has_py, has_ts })
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
    fn mixed_languages_is_ambiguous_error() {
        // A repo declaring both .py and .ts must fail loudly rather than
        // silently tie-break — otherwise local `hm run` and cloud discovery
        // could resolve different languages for the same repo.
        let tmp = setup(&["ci.py", "deploy.ts"]);
        let err = detect_language(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("ambiguous pipeline language"),
            "unexpected error: {msg}"
        );
        assert!(msg.contains(".py") && msg.contains(".ts"), "msg: {msg}");
    }

    #[test]
    fn no_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        // Do NOT create .hm/
        let err = detect_language(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("no .hm/ directory"), "unexpected error: {msg}");
    }

    #[test]
    fn empty_harmont_dir_is_error() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join(".hm")).unwrap();
        let err = detect_language(tmp.path()).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("no .py or .ts files"),
            "unexpected error: {msg}"
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
        // No .hm/ directory at all.
        assert!(!has_pipeline_files(TempDir::new().unwrap().path()));
        // .hm/ exists but declares no .py/.ts files.
        assert!(!has_pipeline_files(setup(&["README.md"]).path()));
    }
}
