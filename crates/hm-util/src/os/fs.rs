//! Atomic, permission-restricted filesystem helpers.
//!
//! The main entry point is [`write_atomic_restricted`]. A synchronous
//! wrapper is available at [`blocking::write_atomic_restricted`] for
//! callers that run inside a tokio runtime but cannot use async
//! (e.g. extism `host_fn` callbacks).
//!
//! Both guarantee that readers observe either the full old contents or
//! the full new contents — never a truncated file — and that Unix
//! file/directory modes are set atomically with creation.

use std::io;
use std::path::Path;

/// Write `contents` to `path` atomically with `file_mode`, ensuring the
/// parent directory exists and is set to `dir_mode`.
///
/// Internally offloads blocking I/O to [`tokio::task::spawn_blocking`].
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
) -> io::Result<()> {
    let dest = path.as_ref().to_owned();
    let contents = contents.as_ref().to_vec();
    let path = dest.clone();

    let tmp_path = tokio::task::spawn_blocking(move || {
        let parent = path.parent().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} has no parent directory", path.display()),
            )
        })?;

        create_dir_with_mode(parent, dir_mode)?;

        let file_name = path
            .file_name()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("{} has no file name", path.display()),
                )
            })?
            .to_os_string();
        let mut tmp_name = file_name;
        tmp_name.push(format!(".tmp.{}", std::process::id()));
        let tmp_path = parent.join(&tmp_name);

        write_file_with_mode(&tmp_path, &contents, file_mode)?;

        io::Result::Ok(tmp_path)
    })
    .await
    .map_err(io::Error::other)??;

    let rename_result = atomic_rename_over(&tmp_path, &dest).await;
    if rename_result.is_err() {
        let _ = tokio::fs::remove_file(&tmp_path).await;
    }
    rename_result
}

/// Atomically replace `to` with `from`.
///
/// On Unix this delegates to [`tokio::fs::rename`] (`rename(2)` — atomic
/// by POSIX guarantee). On Windows this uses `ReplaceFileW` (preserves
/// ACLs and alternate data streams) when the target exists, falling back
/// to `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING` for first-write.
///
/// # Errors
///
/// Returns an error if the rename fails (permission denied, cross-device,
/// source missing, etc.).
pub async fn atomic_rename_over(
    from: impl AsRef<Path>,
    to: impl AsRef<Path>,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        tokio::fs::rename(from.as_ref(), to.as_ref()).await
    }
    #[cfg(windows)]
    {
        let from = from.as_ref().to_owned();
        let to = to.as_ref().to_owned();
        tokio::task::spawn_blocking(move || atomic_rename_over_impl(&from, &to))
            .await
            .map_err(io::Error::other)?
    }
}

/// Remove a file if it exists; silently return `Ok(())` if it does not.
///
/// # Errors
///
/// Returns an error if `remove_file` fails for any reason other than
/// `NotFound`.
pub async fn remove_file_if_exists(path: impl AsRef<Path>) -> io::Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// Platform helpers (private)
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn create_dir_with_mode(dir: &Path, mode: u32) -> io::Result<()> {
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
fn create_dir_with_mode(dir: &Path, _mode: u32) -> io::Result<()> {
    std::fs::create_dir_all(dir)
}

#[cfg(unix)]
fn write_file_with_mode(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
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
fn write_file_with_mode(path: &Path, contents: &[u8], _mode: u32) -> io::Result<()> {
    std::fs::write(path, contents)
}

#[cfg(windows)]
fn atomic_rename_over_impl(from: &Path, to: &Path) -> io::Result<()> {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        MoveFileExW, ReplaceFileW,
        MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        REPLACEFILE_IGNORE_MERGE_ERRORS,
    };

    let from_w = HSTRING::from(from.as_os_str());
    let to_w = HSTRING::from(to.as_os_str());

    if to.exists() {
        let result = unsafe {
            ReplaceFileW(
                &to_w,
                &from_w,
                windows::core::PCWSTR::null(),
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                None,
                None,
            )
        };
        return result.map_err(|e| io::Error::new(io::ErrorKind::Other, e));
    }

    let result = unsafe {
        MoveFileExW(
            &from_w,
            &to_w,
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    result.map_err(|e| io::Error::new(io::ErrorKind::Other, e))
}

// ---------------------------------------------------------------------------
// Blocking wrappers
// ---------------------------------------------------------------------------

/// Synchronous wrappers that shell out to the async API via
/// `tokio::task::block_in_place`. Safe to call from sync contexts
/// that run inside a tokio runtime (e.g. extism `host_fn` callbacks).
pub mod blocking {
    use std::io;
    use std::path::Path;

    fn block_on<F: std::future::Future<Output = io::Result<()>>>(f: F) -> io::Result<()> {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
    }

    /// Blocking counterpart of [`super::write_atomic_restricted`].
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
    ) -> io::Result<()> {
        block_on(super::write_atomic_restricted(path, contents, file_mode, dir_mode))
    }

    /// Blocking counterpart of [`super::remove_file_if_exists`].
    ///
    /// # Errors
    ///
    /// Returns an error if `remove_file` fails for any reason other than
    /// `NotFound`.
    pub fn remove_if_exists(path: impl AsRef<Path>) -> io::Result<()> {
        block_on(super::remove_file_if_exists(path))
    }
}

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::blocking;
    use std::os::unix::fs::PermissionsExt;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn writes_file_and_dir_with_requested_modes() {
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn overwrites_existing_file_preserving_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("creds");
        blocking::write_atomic_restricted(&target, b"v1", 0o600, 0o700).unwrap();
        blocking::write_atomic_restricted(&target, b"v2", 0o600, 0o700).unwrap();

        assert_eq!(std::fs::read(&target).unwrap(), b"v2");
        let mode = std::fs::metadata(&target).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tightens_existing_dir_with_looser_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("loose");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

        let target = dir.join("creds");
        blocking::write_atomic_restricted(&target, b"x", 0o600, 0o700).unwrap();

        let dir_mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dir_mode, 0o700);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn remove_if_exists_is_idempotent() {
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
        super::remove_file_if_exists(&target).await.unwrap();
        std::fs::write(&target, "x").unwrap();
        super::remove_file_if_exists(&target).await.unwrap();
        assert!(!target.exists());
    }

    #[tokio::test]
    async fn atomic_rename_over_replaces_target() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("target");
        std::fs::write(&dst, b"old").unwrap();
        std::fs::write(&src, b"new").unwrap();

        super::atomic_rename_over(&src, &dst).await.unwrap();

        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
        assert!(!src.exists(), "source should be gone after rename");
    }

    #[tokio::test]
    async fn atomic_rename_over_works_when_target_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("source");
        let dst = tmp.path().join("target");
        std::fs::write(&src, b"new").unwrap();

        super::atomic_rename_over(&src, &dst).await.unwrap();

        assert_eq!(std::fs::read(&dst).unwrap(), b"new");
        assert!(!src.exists());
    }
}
