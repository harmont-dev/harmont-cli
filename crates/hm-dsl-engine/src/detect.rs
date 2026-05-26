use std::path::Path;

use anyhow::{Context, bail};

use crate::DslLanguage;

/// Detect the DSL language used in a project by scanning `.harmont/` for file
/// extensions.
///
/// # Errors
///
/// - The `.harmont/` directory does not exist.
/// - No `.py` or `.ts` files are found inside `.harmont/`.
/// - Both `.py` and `.ts` files are present (mixed languages).
pub fn detect_language(repo_root: &Path) -> anyhow::Result<DslLanguage> {
    let harmont_dir = repo_root.join(".harmont");

    if !harmont_dir.is_dir() {
        bail!("no .harmont/ directory found in {}", repo_root.display());
    }

    let entries = std::fs::read_dir(&harmont_dir)
        .with_context(|| format!("failed to read {}", harmont_dir.display()))?;

    let mut has_py = false;
    let mut has_ts = false;

    for entry in entries {
        let entry = entry?;
        let path = entry.path();

        match path.extension().and_then(|e| e.to_str()) {
            Some("py") => has_py = true,
            Some("ts") => has_ts = true,
            _ => {}
        }
    }

    match (has_py, has_ts) {
        // When both languages are present, prefer TypeScript.
        (_, true) => Ok(DslLanguage::TypeScript),
        (true, false) => Ok(DslLanguage::Python),
        (false, false) => bail!("no .py or .ts files found in {}", harmont_dir.display()),
    }
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
}
