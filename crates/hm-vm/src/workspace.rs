//! Host-side workspace utilities for COW build directories.

use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, ensure};

/// Create a copy-on-write clone of `src` contents into `dst`.
///
/// macOS: `cp -cpR` (APFS clonefile; `cp -c` itself falls back to
/// `copyfile(2)` when cloning is unsupported, so no manual retry is
/// needed). Linux: `cp --reflink=auto -a` (COW on btrfs/XFS, full copy
/// on ext4). Symlinks are copied as symlinks; mode and mtime are
/// preserved (incremental build tools depend on mtimes).
///
/// # Errors
///
/// Returns an error if `cp` cannot be spawned or exits with a non-zero
/// status; `cp`'s stderr is captured into the error rather than leaked
/// to the terminal.
pub fn cow_copy(src: &Path, dst: &Path) -> Result<()> {
    let src_dot = format!("{}/.", src.display());

    let mut cmd = Command::new("cp");
    if cfg!(target_os = "macos") {
        cmd.args(["-cpR", &src_dot]);
    } else {
        cmd.args(["--reflink=auto", "-a", &src_dot]);
    }
    cmd.arg(dst);

    let output = cmd
        .output()
        .with_context(|| format!("spawning cp: {} -> {}", src.display(), dst.display()))?;

    ensure!(
        output.status.success(),
        "cp {} -> {} exited with {}: {}",
        src.display(),
        dst.display(),
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn cow_copy_produces_independent_clone() {
        let src = tempdir().unwrap();
        fs::write(src.path().join("file.txt"), "original").unwrap();
        fs::create_dir(src.path().join("sub")).unwrap();
        fs::write(src.path().join("sub/nested.txt"), "nested").unwrap();

        let dst = tempdir().unwrap();
        cow_copy(src.path(), dst.path()).unwrap();

        assert_eq!(
            fs::read_to_string(dst.path().join("file.txt")).unwrap(),
            "original"
        );
        assert_eq!(
            fs::read_to_string(dst.path().join("sub/nested.txt")).unwrap(),
            "nested"
        );

        fs::write(dst.path().join("file.txt"), "modified").unwrap();
        assert_eq!(
            fs::read_to_string(src.path().join("file.txt")).unwrap(),
            "original"
        );
    }

    #[test]
    fn cow_copy_empty_dir() {
        let src = tempdir().unwrap();
        let dst = tempdir().unwrap();
        cow_copy(src.path(), dst.path()).unwrap();
    }
}
