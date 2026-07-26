//! Filesystem helpers.
//!
//! Two families live here:
//!
//! * [`write_create_all`] — a convenience wrapper that scaffolds parent
//!   directories before a plain write, with no permission guarantees.
//! * [`write_atomic`] (and its [`blocking`] counterpart) — an atomic writer
//!   that also controls who may read the result via [`Privacy`]. Readers
//!   observe either the full old contents or the full new contents, never a
//!   truncated file.
//!
//! ## Privacy on Windows
//!
//! [`Privacy`] maps to Unix permission bits. On Windows it is currently a
//! **no-op** — Windows ACLs are not yet enforced, so a `Private` file is no
//! more restricted than a `Public` one. The intent is still recorded in the
//! type, so a real ACL implementation can drop in behind `#[cfg(windows)]`
//! without touching any call site.

use std::io;
use std::path::Path;

/// Who may read a file (or directory) that hm writes.
///
/// Carries intent rather than raw mode bits: the file- and directory-mode
/// mapping is applied internally, so a secrets file can never accidentally
/// land with a directory's permissions. See the [module docs](self) for the
/// Windows caveat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    /// Owner-only. Unix file `0o600`, directory `0o700`. Use for secrets
    /// (e.g. credentials).
    Private,
    /// World-readable, owner-writable. Unix file `0o644`, directory `0o755`.
    Public,
}

impl Privacy {
    /// Unix permission bits for a *file* at this privacy level.
    #[cfg(unix)]
    const fn file_mode(self) -> u32 {
        match self {
            Self::Private => 0o600,
            Self::Public => 0o644,
        }
    }

    /// Unix permission bits for a *directory* at this privacy level.
    #[cfg(unix)]
    const fn dir_mode(self) -> u32 {
        match self {
            Self::Private => 0o700,
            Self::Public => 0o755,
        }
    }
}

/// Write `contents` to `path`, creating any missing parent directories first.
///
/// Joins [`std::fs::create_dir_all`] on the parent and [`std::fs::write`] into
/// one call, so callers scaffolding a file into a not-yet-existing directory
/// don't repeat the parent-creation dance. An existing file is overwritten.
///
/// This makes no permission guarantees — reach for [`write_atomic`] when the
/// result must be owner-only or written atomically.
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

/// Atomically write `contents` to `path` at the given [`Privacy`], ensuring the
/// parent directory exists.
///
/// The parent directory is always created owner-only (`0o700` on Unix); only
/// the file's readability is controlled by `file`. The write goes to a
/// tempfile in the same directory and is `rename`d over `path`, so a reader
/// never sees a partial file.
///
/// # Errors
///
/// Returns an error if `path` has no parent or no file-name component, the
/// parent directory cannot be created or chmod'd, the tempfile cannot be
/// opened or written, or the final `rename` over `path` fails.
pub async fn write_atomic(
    path: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
    file: Privacy,
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

    create_dir_private(&parent).await?;

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

    write_file_private(&tmp_path, &contents, file).await?;

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

/// Create `dir` (and any missing parents) owner-only.
///
/// On Unix the directory is created at, or chmod'd to, `0o700`. On Windows
/// this is a plain recursive create (see the [module docs](self) on Windows
/// privacy).
async fn create_dir_private(dir: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = Privacy::Private.dir_mode();
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
        Ok(())
    }

    #[cfg(windows)]
    {
        tokio::fs::create_dir_all(dir).await
    }
}

/// Write `contents` to `path` at the file mode implied by `privacy`.
///
/// On Unix the file is opened with the exact mode so the bytes never exist at
/// a laxer permission. On Windows the privacy is a no-op (see [module
/// docs](self)).
async fn write_file_private(path: &Path, contents: &[u8], privacy: Privacy) -> io::Result<()> {
    #[cfg(unix)]
    {
        use tokio::io::AsyncWriteExt;
        let mut opts = tokio::fs::OpenOptions::new();
        opts.write(true)
            .create(true)
            .truncate(true)
            .mode(privacy.file_mode());
        let mut f = opts.open(path).await?;
        f.write_all(contents).await?;
        f.sync_all().await?;
    }

    #[cfg(windows)]
    {
        let _ = privacy;
        tokio::fs::write(path, contents).await?;
    }

    Ok(())
}

/// Synchronous wrappers that shell out to the async API via
/// `tokio::task::block_in_place`. Safe to call from sync contexts
/// that run inside a tokio runtime.
pub mod blocking {
    use super::Privacy;
    use std::io;
    use std::path::Path;

    fn block_on<F: std::future::Future<Output = io::Result<()>>>(f: F) -> io::Result<()> {
        tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(f))
    }

    /// Blocking counterpart of [`super::write_atomic`].
    ///
    /// See the [module-level documentation](super) for semantics.
    ///
    /// # Errors
    ///
    /// Returns an error if `path` has no parent or no file-name component,
    /// the parent directory cannot be created or chmod'd, the tempfile cannot
    /// be opened or written, or the final `rename` over `path` fails.
    pub fn write_atomic(
        path: impl AsRef<Path>,
        contents: impl AsRef<[u8]>,
        file: Privacy,
    ) -> io::Result<()> {
        block_on(super::write_atomic(path, contents, file))
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

#[cfg(all(test, unix))]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod unix_privacy_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn mode_of(path: &Path) -> u32 {
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777
    }

    /// A `Private` file must land at exactly 0o600, in a 0o700 dir.
    #[tokio::test]
    async fn private_file_is_0600_in_dir_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hm");
        let file = dir.join("credentials.toml");

        write_atomic(&file, b"token = \"hunter2\"\n", Privacy::Private)
            .await
            .unwrap();

        assert_eq!(mode_of(&file), 0o600, "file mode must be 0o600");
        assert_eq!(mode_of(&dir), 0o700, "dir mode must be 0o700");
    }

    /// A `Public` file lands at 0o644, still in a 0o700 dir.
    #[tokio::test]
    async fn public_file_is_0644_in_dir_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hm");
        let file = dir.join("config.toml");

        write_atomic(&file, b"key = 1\n", Privacy::Public)
            .await
            .unwrap();

        assert_eq!(mode_of(&file), 0o644, "file mode must be 0o644");
        assert_eq!(mode_of(&dir), 0o700, "dir mode must be 0o700");
    }

    /// Overwriting a secret must preserve 0o600 (guards the tempfile +
    /// atomic-rename path against perm drift).
    #[tokio::test]
    async fn rewrite_preserves_private_mode() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("credentials.toml");
        write_atomic(&file, b"a", Privacy::Private).await.unwrap();
        write_atomic(&file, b"bb", Privacy::Private).await.unwrap();
        assert_eq!(mode_of(&file), 0o600, "file mode must stay 0o600");
    }
}
