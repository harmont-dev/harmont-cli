//! Atomic, permission-restricted filesystem helpers.
//!
//! The main entry point is [`write_atomic_restricted`] (async) or
//! [`blocking::write_atomic_restricted`] (sync). Both guarantee that
//! readers observe either the full old contents or the full new
//! contents — never a truncated file — and that Unix file/directory
//! modes are set atomically with creation.

use std::path::Path;

use anyhow::{Context, Result};

// ---------------------------------------------------------------------------
// Private sync core
// ---------------------------------------------------------------------------

fn write_atomic_restricted_sync(
    path: &Path,
    contents: &[u8],
    file_mode: u32,
    dir_mode: u32,
) -> Result<()> {
    let parent = path
        .parent()
        .with_context(|| format!("{} has no parent directory", path.display()))?;

    create_dir_with_mode_sync(parent, dir_mode)
        .with_context(|| format!("creating {}", parent.display()))?;

    let file_name = path
        .file_name()
        .with_context(|| format!("{} has no file name", path.display()))?
        .to_os_string();
    let mut tmp_name = file_name;
    tmp_name.push(format!(".tmp.{}", std::process::id()));
    let tmp_path = parent.join(&tmp_name);

    write_file_with_mode_sync(&tmp_path, contents, file_mode)
        .with_context(|| format!("writing {}", tmp_path.display()))?;

    let persist_result = std::fs::rename(&tmp_path, path)
        .with_context(|| format!("renaming {} -> {}", tmp_path.display(), path.display()));

    if persist_result.is_err() {
        let _ = std::fs::remove_file(&tmp_path);
    }
    persist_result?;

    Ok(())
}

fn remove_if_exists_sync(path: &Path) -> Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("removing {}", path.display())),
    }
}

#[cfg(unix)]
fn create_dir_with_mode_sync(dir: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    if dir.exists() {
        let current = std::fs::metadata(dir)?.permissions().mode() & 0o777;
        if current != mode {
            std::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode))?;
        }
    } else {
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(mode)
            .create(dir)?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn create_dir_with_mode_sync(dir: &Path, _mode: u32) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn write_file_with_mode_sync(path: &Path, contents: &[u8], mode: u32) -> std::io::Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    f.write_all(contents)?;
    f.sync_all()?;
    Ok(())
}

#[cfg(not(unix))]
fn write_file_with_mode_sync(path: &Path, contents: &[u8], _mode: u32) -> std::io::Result<()> {
    std::fs::write(path, contents)
}

// ---------------------------------------------------------------------------
// Public async API
// ---------------------------------------------------------------------------

/// Write `contents` to `path` atomically with `file_mode`, ensuring the
/// parent directory exists and is set to `dir_mode`.
///
/// This is the async counterpart of [`blocking::write_atomic_restricted`];
/// the blocking I/O is offloaded to [`tokio::task::spawn_blocking`].
///
/// # Errors
///
/// Returns an error if `path` has no parent or no file-name component,
/// the parent directory cannot be created or chmod'd to `dir_mode`, the
/// tempfile cannot be opened with `file_mode` or written, or the final
/// `rename` over `path` fails.
pub async fn write_atomic_restricted(
    path: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
    file_mode: u32,
    dir_mode: u32,
) -> Result<()> {
    let path = path.as_ref().to_owned();
    let contents = contents.as_ref().to_vec();
    tokio::task::spawn_blocking(move || {
        write_atomic_restricted_sync(&path, &contents, file_mode, dir_mode)
    })
    .await
    .context("write_atomic_restricted task panicked")?
}

/// Remove a file if it exists; silently return `Ok(())` if it does not.
///
/// This is the async counterpart of [`blocking::remove_if_exists`];
/// the blocking I/O is offloaded to [`tokio::task::spawn_blocking`].
///
/// # Errors
///
/// Returns an error if `remove_file` fails for any reason other than
/// `NotFound`.
pub async fn remove_if_exists(path: impl AsRef<Path>) -> Result<()> {
    let path = path.as_ref().to_owned();
    tokio::task::spawn_blocking(move || remove_if_exists_sync(&path))
        .await
        .context("remove_if_exists task panicked")?
}

// ---------------------------------------------------------------------------
// Public blocking module
// ---------------------------------------------------------------------------

/// Synchronous (blocking) wrappers for callers that cannot use async,
/// such as extism `host_fn` callbacks.
pub mod blocking {
    use std::path::Path;

    use anyhow::Result;

    /// Write `contents` to `path` atomically with `file_mode`, ensuring the
    /// parent directory exists and is set to `dir_mode`.
    ///
    /// See the [module-level documentation](super) for semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` has no parent or no file-name component,
    /// the parent directory cannot be created or chmod'd to `dir_mode`, the
    /// tempfile cannot be opened with `file_mode` or written, or the final
    /// `rename` over `path` fails.
    pub fn write_atomic_restricted(
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
        file_mode: u32,
        dir_mode: u32,
    ) -> Result<()> {
        super::write_atomic_restricted_sync(path.as_ref(), contents.as_ref(), file_mode, dir_mode)
    }

    /// Remove a file if it exists; silently return `Ok(())` if it does not.
    ///
    /// # Errors
    ///
    /// Returns an error if `remove_file` fails for any reason other than
    /// `NotFound`.
    pub fn remove_if_exists(path: impl AsRef<Path>) -> Result<()> {
        super::remove_if_exists_sync(path.as_ref())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::blocking;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn writes_file_and_dir_with_requested_modes() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("sub").join("creds");
        blocking::write_atomic_restricted(&target, b"hello", 0o600, 0o700).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        let file_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        let dir_mode = std::fs::metadata(target.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(
            file_mode, 0o600,
            "file mode must be 0o600, got {file_mode:o}"
        );
        assert_eq!(dir_mode, 0o700, "dir mode must be 0o700, got {dir_mode:o}");
    }

    #[test]
    fn overwrites_existing_file_preserving_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("creds");
        blocking::write_atomic_restricted(&target, b"v1", 0o600, 0o700).unwrap();
        blocking::write_atomic_restricted(&target, b"v2", 0o600, 0o700).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"v2");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn tightens_existing_dir_with_looser_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("loose");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let target = dir.join("creds");
        blocking::write_atomic_restricted(&target, b"x", 0o600, 0o700).unwrap();

        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
    }

    #[test]
    fn remove_if_exists_is_idempotent() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nothing");
        blocking::remove_if_exists(&target).unwrap();
        std::fs::write(&target, "x").unwrap();
        blocking::remove_if_exists(&target).unwrap();
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn async_write_atomic_restricted() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("sub").join("async_creds");
        super::write_atomic_restricted(&target, b"async hello", 0o600, 0o700)
            .await
            .unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"async hello");
        let file_mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(file_mode, 0o600);
    }

    #[tokio::test]
    async fn async_remove_if_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nothing");
        super::remove_if_exists(&target).await.unwrap();
        std::fs::write(&target, "x").unwrap();
        super::remove_if_exists(&target).await.unwrap();
        assert!(!target.exists());
    }
}
