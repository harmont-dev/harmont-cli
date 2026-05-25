use std::path::Path;

use include_dir::{Dir, include_dir};

/// The `harmont` Python package source tree (`dsls/harmont-py/harmont/`).
pub(crate) static HARMONT_PY: Dir<'_> =
    include_dir!("$CARGO_MANIFEST_DIR/../../dsls/harmont-py/harmont");

/// Pre-compiled ESM bundle of harmont-ts (main entry).
pub(crate) const HARMONT_TS_INDEX: &str =
    include_str!(concat!(env!("OUT_DIR"), "/harmont-index.mjs"));

/// Pre-compiled ESM bundle of harmont-ts toolchains subpath.
pub(crate) const HARMONT_TS_TOOLCHAINS: &str =
    include_str!(concat!(env!("OUT_DIR"), "/harmont-toolchains.mjs"));

/// Extract an embedded directory tree to disk.
pub(crate) fn extract_to(dir: &Dir<'_>, target: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(target)
        .map_err(|e| anyhow::anyhow!("creating directory {}: {e}", target.display()))?;
    dir.extract(target)
        .map_err(|e| anyhow::anyhow!("extracting to {}: {e}", target.display()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
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
    fn ts_index_bundle_is_not_empty() {
        assert!(
            HARMONT_TS_INDEX.len() > 100,
            "bundle should be non-trivial, got {} bytes",
            HARMONT_TS_INDEX.len()
        );
    }

    #[test]
    fn ts_toolchains_bundle_is_not_empty() {
        assert!(
            HARMONT_TS_TOOLCHAINS.len() > 100,
            "bundle should be non-trivial, got {} bytes",
            HARMONT_TS_TOOLCHAINS.len()
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
}
