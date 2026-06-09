//! Source-archive helpers shared between remote and local run modes.
//!
//! Walks a directory respecting `.gitignore` and produces a `.tar.gz`.
//! Local mode pipes the result into a chain-root container's stdin so
//! steps see the user's tree under `/workspace`. Remote mode hashes the
//! same bytes and ships them as a base64 blob in the build request.

use std::io::Write;
use std::path::Path;

use anyhow::{Context, Result};
use flate2::Compression;
use flate2::write::GzEncoder;
use ignore::WalkBuilder;
use tar::Builder as TarBuilder;

/// Build a tar.gz archive of `source_dir` (respecting .gitignore) and
/// return the resulting bytes. Excludes the literal `.git` directory.
///
/// # Errors
///
/// Returns the same errors as [`write_archive`].
pub fn build_archive_bytes(source_dir: &Path) -> Result<Vec<u8>> {
    let mut buf: Vec<u8> = Vec::new();
    write_archive(source_dir, &mut buf)?;
    Ok(buf)
}

/// Write a tar.gz archive of `source_dir` into `w`.
///
/// # Errors
///
/// Returns an error if walking the source tree surfaces an I/O or
/// permission error, if a file cannot be appended to the archive, or
/// if the gzip stream cannot be finalised on the destination writer.
pub fn write_archive(source_dir: &Path, w: impl Write) -> Result<()> {
    let encoder = GzEncoder::new(w, Compression::fast());
    let mut archive = TarBuilder::new(encoder);

    // `WalkBuilder` defaults to `hidden(true)`, which silently drops
    // every dotfile — `.eslintrc.json`, `.ocamlformat`, `.gitignore`
    // overrides per-example, etc. We need those in the archive shipped
    // to the container, so flip `hidden(false)`. The literal `.git`
    // and `.hm` directories are still excluded via `filter_entry`
    // — `.git` is repository bookkeeping; `.hm` holds the
    // pipeline-render entry point (already executed host-side) and
    // its `__pycache__`, both of which would otherwise leak into the
    // workspace and trip up project-level tools (e.g. ruff format
    // walking every `.py` file under /workspace).
    let walker = WalkBuilder::new(source_dir)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .filter_entry(|entry: &ignore::DirEntry| {
            let name = entry.file_name().to_string_lossy();
            name != ".git" && name != ".hm"
        })
        .build();

    for entry in walker {
        let entry: ignore::DirEntry = entry.context("walking source directory")?;
        let entry_path = entry.path();
        if entry_path.is_file() {
            let relative = entry_path.strip_prefix(source_dir).unwrap_or(entry_path);
            archive
                .append_path_with_name(entry_path, relative)
                .with_context(|| format!("adding {}", entry_path.display()))?;
        }
    }
    archive
        .into_inner()
        .context("finishing gzip stream")?
        .finish()
        .context("finalizing gzip")?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn build_archive_emits_nonempty_gzip_for_simple_tree() {
        let tmp = tempfile::tempdir().unwrap();
        fs::write(tmp.path().join("hello.txt"), b"hi").unwrap();
        let bytes = build_archive_bytes(tmp.path()).unwrap();
        // gzip magic 0x1f 0x8b
        assert!(bytes.len() > 2);
        assert_eq!(&bytes[..2], &[0x1f, 0x8b]);
    }

    #[test]
    fn build_archive_skips_dot_git() {
        use flate2::read::GzDecoder;
        use std::io::Read;

        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir(tmp.path().join(".git")).unwrap();
        fs::write(tmp.path().join(".git/HEAD"), b"ref: refs/heads/main").unwrap();
        fs::write(tmp.path().join("kept.txt"), b"k").unwrap();

        let bytes = build_archive_bytes(tmp.path()).unwrap();
        // Inflate and inspect entries.
        let mut gz = GzDecoder::new(&bytes[..]);
        let mut tar_bytes = Vec::new();
        gz.read_to_end(&mut tar_bytes).unwrap();
        let mut ar = tar::Archive::new(&tar_bytes[..]);
        let names: Vec<String> = ar
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().display().to_string())
            .collect();
        assert!(names.iter().any(|n| n == "kept.txt"), "got: {names:?}");
        assert!(
            !names.iter().any(|n| n.starts_with(".git")),
            "got: {names:?}"
        );
    }
}
