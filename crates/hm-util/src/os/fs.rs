//! Filesystem helpers.
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

/// Unix mode bits for a file (e.g. `0o600`).
///
/// A distinct newtype from [`DirMode`] so the file- and directory-mode
/// arguments of [`write_atomic_restricted`] cannot be transposed: passing
/// them in the wrong order is a compile error rather than a silent
/// security regression (a secrets file landing at `0o700`, say).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileMode(pub u32);

/// Unix mode bits for a directory (e.g. `0o700`).
///
/// See [`FileMode`] for why this is a distinct newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirMode(pub u32);

/// Write `contents` to `path` atomically with `file`, ensuring the
/// parent directory exists and is set to `dir`.
///
/// # Errors
///
/// Returns an error if `path` has no parent or no file-name component,
/// the parent directory cannot be created or chmod'd to `dir`, the
/// tempfile cannot be opened with `file` or written, or the final
/// `rename` over `path` fails.
pub async fn write_atomic_restricted(
    path: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
    file: FileMode,
    dir: DirMode,
) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    let contents = contents.as_ref().to_vec();

    let parent = path
        .parent()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} has no parent directory", path.display()),
            )
        })?
        .to_owned();

    create_dir_with_mode(&parent, dir.0).await?;

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

    write_file_with_mode(&tmp_path, &contents, file.0).await?;

    let rename_result = atomic_rename_over(&tmp_path, &path).await;
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
pub async fn atomic_rename_over(from: impl AsRef<Path>, to: impl AsRef<Path>) -> io::Result<()> {
    #[cfg(unix)]
    {
        tokio::fs::rename(from.as_ref(), to.as_ref()).await
    }
    #[cfg(windows)]
    {
        fn atomic_rename_over_impl(from: &Path, to: &Path) -> io::Result<()> {
            use windows::Win32::Storage::FileSystem::{
                MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW,
                REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW,
            };
            use windows::core::HSTRING;

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

#[cfg(unix)]
async fn create_dir_with_mode(dir: &Path, mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        match tokio::fs::metadata(dir).await {
            Ok(meta) => {
                let current = meta.permissions().mode() & 0o777;
                if current != mode {
                    tokio::fs::set_permissions(dir, std::fs::Permissions::from_mode(mode)).await?;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                let mut builder = tokio::fs::DirBuilder::new();
                builder.recursive(true).mode(mode);
                builder.create(dir).await?;
            }
            Err(e) => return Err(e),
        }
    }

    #[cfg(windows)]
    {
        tokio::fs::create_dir_all(dir).await
    }
    Ok(())
}

async fn write_file_with_mode(path: &Path, contents: &[u8], mode: u32) -> io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::io::AsyncWriteExt;
        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true).mode(mode);
        let mut f = opts.open(path).await?;
        f.write_all(contents).await?;
        f.sync_all().await?;
    }

    #[cfg(windows)]
    {
        tokio::fs::write(path, contents).await
    }

    Ok(())
}

/// Synchronous wrappers that shell out to the async API via
/// `tokio::task::block_in_place`. Safe to call from sync contexts
/// that run inside a tokio runtime (e.g. extism `host_fn` callbacks).
pub mod blocking {
    use super::{DirMode, FileMode};
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
    /// the parent directory cannot be created or chmod'd to `dir`, the
    /// tempfile cannot be opened with `file` or written, or the final
    /// `rename` over `path` fails.
    pub fn write_atomic_restricted(
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
        file: FileMode,
        dir: DirMode,
    ) -> io::Result<()> {
        block_on(super::write_atomic_restricted(path, contents, file, dir))
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
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// Credentials (and any secret) must land at exactly 0o600, in a 0o700 dir.
    #[tokio::test]
    async fn writes_file_0600_in_dir_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hm");
        let file = dir.join("credentials.toml");

        write_atomic_restricted(
            &file,
            b"token = \"hunter2\"\n",
            FileMode(0o600),
            DirMode(0o700),
        )
        .await
        .unwrap();

        let fmode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(fmode, 0o600, "file mode must be 0o600, got {fmode:o}");
        let dmode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(dmode, 0o700, "dir mode must be 0o700, got {dmode:o}");
    }

    /// Overwriting an existing secret must preserve 0o600 (guards the
    /// temp-file + atomic-rename path against perm drift).
    #[tokio::test]
    async fn rewrite_preserves_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("credentials.toml");
        write_atomic_restricted(&file, b"a", FileMode(0o600), DirMode(0o700))
            .await
            .unwrap();
        write_atomic_restricted(&file, b"bb", FileMode(0o600), DirMode(0o700))
            .await
            .unwrap();
        let fmode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(fmode, 0o600, "file mode must stay 0o600, got {fmode:o}");
    }
}
