use std::path::Path;

use include_dir::{Dir, include_dir};

/// The `harmont` Python package source tree (`harmont-py/harmont/`).
pub(crate) static HARMONT_PY: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/harmont-py/harmont");

/// Extract an embedded directory tree to disk.
pub(crate) fn extract_to(dir: &Dir<'_>, target: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)
        .map_err(|e| anyhow::anyhow!("creating directory {}: {e}", target.display()))?;
    dir.extract(target)
        .map_err(|e| anyhow::anyhow!("extracting to {}: {e}", target.display()))
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

    // The pipeline module is private (underscore-prefixed); the public
    // surface is re-exported from `__init__.py`.
    #[rstest]
    #[case::init("__init__.py")]
    #[case::pipeline("_pipeline.py")]
    #[case::py_typed("py.typed")]
    fn harmont_py_contains(#[case] name: &str) {
        assert!(HARMONT_PY.get_file(name).is_some());
    }

    #[rstest]
    fn extract_harmont_py_creates_files() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("harmont");
        extract_to(&HARMONT_PY, &target).expect("extract");
        assert!(target.join("__init__.py").exists());
        assert!(target.join("_pipeline.py").exists());
    }
}
