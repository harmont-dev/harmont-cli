//! Filesystem helpers.

use std::io;
use std::path::Path;

/// Who may read a file (or directory) that hm writes.
///
/// On Windows this is not enforced nor respected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Privacy {
    /// Owner-only. Unix file `0o600`, directory `0o700`. Use for secrets (e.g. credentials).
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
/// This makes no permission guarantees — reach for [`write_atomic`] when the result must be
/// owner-only or written atomically.
///
/// # Errors
/// Returns the underlying [`io::Error`] if a parent directory cannot be created or the file cannot
/// be written.
pub async fn write_create_all(
    path: impl AsRef<Path>,
    contents: impl AsRef<[u8]>,
) -> io::Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, contents).await
}

/// Atomically write `contents` to `path` at the given [`Privacy`], ensuring the parent directory
/// exists at that same privacy.
///
/// The parent directory is created (and, if it already exists, chmod'd) to match `file`: `0o700`
/// for [`Privacy::Private`], `0o755` for [`Privacy::Public`] on Unix.
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

    create_dir_at(&parent, file).await?;

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
pub async fn remove_file_if_exists(path: impl AsRef<Path>) -> io::Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Create `dir` (and any missing parents) at `privacy`'s directory mode.
async fn create_dir_at(dir: &Path, privacy: Privacy) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = privacy.dir_mode();
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
        let _ = privacy;
        tokio::fs::create_dir_all(dir).await
    }
}

/// Write `contents` to `path` at the file mode implied by `privacy`.
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

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::flat("file.txt")]
    #[case::one_level("sub/file.txt")]
    #[case::deeply_nested("a/b/c/file.txt")]
    #[tokio::test]
    async fn writes_file_creating_missing_parents(#[case] rel: &str) {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join(rel);

        write_create_all(&path, b"hello").await.unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }

    #[rstest]
    #[tokio::test]
    async fn overwrites_an_existing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("file.txt");

        write_create_all(&path, b"first").await.unwrap();
        write_create_all(&path, b"second").await.unwrap();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "second");
    }

    #[rstest]
    #[tokio::test]
    async fn propagates_io_error_when_parent_is_a_file() {
        let tmp = tempfile::tempdir().unwrap();
        // A *file* sits where the target's parent directory would go, so
        // creating the parent must fail with an OS error.
        let blocker = tmp.path().join("blocker");
        std::fs::write(&blocker, b"x").unwrap();
        let target = blocker.join("child.txt");

        assert!(write_create_all(&target, b"data").await.is_err());
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

    /// A `Public` file lands at 0o644, in a matching 0o755 dir.
    #[tokio::test]
    async fn public_file_is_0644_in_dir_0755() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("hm");
        let file = dir.join("config.toml");

        write_atomic(&file, b"key = 1\n", Privacy::Public)
            .await
            .unwrap();

        assert_eq!(mode_of(&file), 0o644, "file mode must be 0o644");
        assert_eq!(mode_of(&dir), 0o755, "dir mode must be 0o755");
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
