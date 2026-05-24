//! Compile-time embedded Python source trees.
//!
//! The [`include_dir`] proc-macro bakes directory contents into the binary so
//! the WASI Python engine can materialise them into a scratch directory without
//! requiring filesystem access to the original source tree.

use std::path::Path;

use include_dir::{include_dir, Dir};

/// The `harmont` Python package source tree
/// (`dsls/harmont-py/harmont/`).
pub static HARMONT_PY: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/../../dsls/harmont-py/harmont");

/// Vendored third-party Python packages required by `harmont`.
///
/// Contains `croniter`, `dateutil` (python-dateutil), and `six` — everything
/// the DSL needs at runtime.
pub static VENDOR_PACKAGES: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/vendor");

/// Extract an embedded directory tree to disk.
///
/// Creates `target` (and its parents) if it does not already exist, then
/// writes every file from `dir` under it.
///
/// # Errors
///
/// Returns an error if any file or directory cannot be created under `target`.
pub fn extract_to(dir: &Dir<'_>, target: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target).map_err(|e| {
        anyhow::anyhow!("creating target directory {}: {e}", target.display())
    })?;
    dir.extract(target)
        .map_err(|e| anyhow::anyhow!("extracting embedded sources to {}: {e}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmont_py_contains_init() {
        assert!(HARMONT_PY.get_file("__init__.py").is_some());
    }

    #[test]
    fn harmont_py_contains_pipeline() {
        assert!(HARMONT_PY.get_file("pipeline.py").is_some());
    }

    #[test]
    fn vendor_contains_croniter() {
        assert!(
            VENDOR_PACKAGES.get_file("croniter/__init__.py").is_some(),
            "vendor must contain croniter package with __init__.py",
        );
    }

    #[test]
    fn extract_harmont_py_creates_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("harmont");
        extract_to(&HARMONT_PY, &target).expect("extract");
        assert!(target.join("__init__.py").exists());
        assert!(target.join("pipeline.py").exists());
    }

    #[test]
    fn extract_vendor_creates_croniter() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("vendor");
        extract_to(&VENDOR_PACKAGES, &target).expect("extract");
        assert!(target.join("croniter").join("__init__.py").exists());
    }
}
