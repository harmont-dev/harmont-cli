//! Filesystem helpers.

use std::io;
use std::path::Path;

/// Write `contents` to `path`, creating any missing parent directories first.
///
/// Joins [`std::fs::create_dir_all`] on the parent and [`std::fs::write`] into
/// one call, so callers scaffolding a file into a not-yet-existing directory
/// don't repeat the parent-creation dance. An existing file is overwritten.
///
/// # Errors
/// Returns the underlying [`io::Error`] if a parent directory cannot be created
/// or the file cannot be written.
pub fn write_create_all(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::flat("file.txt")]
    #[case::one_level("sub/file.txt")]
    #[case::deeply_nested("a/b/c/file.txt")]
    fn writes_file_creating_missing_parents(#[case] rel: &str) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(rel);

        write_create_all(&path, b"hello").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[rstest]
    fn overwrites_an_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("file.txt");

        write_create_all(&path, b"first").unwrap();
        write_create_all(&path, b"second").unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    }

    #[rstest]
    fn propagates_io_error_when_parent_is_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        // A *file* sits where the target's parent directory would go, so
        // creating the parent must fail with an OS error.
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let target = blocker.join("child.txt");

        assert!(write_create_all(&target, b"data").is_err());
    }
}
