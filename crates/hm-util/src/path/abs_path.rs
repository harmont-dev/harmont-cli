//! A path that is guaranteed to be absolute.
//!
//! [`AbsPath`] / [`AbsPathBuf`] mirror the [`Path`] / [`PathBuf`] split from
//! `std`, adding the invariant that the path is absolute.

use std::borrow::Borrow;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Components, Path, PathBuf};

/// A borrowed path that is guaranteed to be absolute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(derive_more::AsRef)]
#[as_ref(forward)]
pub struct AbsPath<'a>(&'a Path);

impl<'a> AbsPath<'a> {
    /// Wrap `path` if it is absolute, otherwise return `None`.
    pub fn new<P: AsRef<Path> + ?Sized>(path: &'a P) -> Option<Self> {
        let p = path.as_ref();
        if p.is_absolute() {
            Some(Self(p))
        } else {
            None
        }
    }

    /// Join a path component onto this absolute path.
    ///
    /// The result is always absolute: if `tail` is absolute it replaces `self`
    /// (same as [`Path::join`]); otherwise it is appended.
    #[must_use]
    pub fn join<P: AsRef<Path>>(&self, tail: P) -> AbsPathBuf {
        AbsPathBuf(self.0.join(tail))
    }

    /// Return the parent directory, if any, as an [`AbsPath`].
    #[must_use]
    pub fn parent(&self) -> Option<AbsPath<'a>> {
        self.0.parent().map(|p| Self(p))
    }

    /// The final component of this path, if any.
    #[must_use]
    pub fn file_name(&self) -> Option<&OsStr> {
        self.0.file_name()
    }

    /// An iterator over the components of this path.
    #[must_use]
    pub fn components(&self) -> Components<'_> {
        self.0.components()
    }

    /// Tests whether this path points to an existing entity on disk.
    #[must_use]
    pub fn exists(&self) -> bool {
        self.0.exists()
    }

    /// Tests whether this path points to a directory.
    #[must_use]
    pub fn is_dir(&self) -> bool {
        self.0.is_dir()
    }

    /// Tests whether this path points to a regular file.
    #[must_use]
    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }

    /// Yield the underlying [`Path`].
    #[must_use]
    pub fn as_path(&self) -> &Path {
        self.0
    }

    /// Convert to an owned [`AbsPathBuf`].
    #[must_use]
    pub fn to_abs_path_buf(&self) -> AbsPathBuf {
        AbsPathBuf(self.0.to_path_buf())
    }
}

impl fmt::Display for AbsPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.display(), f)
    }
}

impl<'a> From<&'a AbsPathBuf> for AbsPath<'a> {
    fn from(buf: &'a AbsPathBuf) -> Self {
        buf.as_abs_path()
    }
}

/// An owned path that is guaranteed to be absolute.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(derive_more::Deref, derive_more::AsRef, derive_more::Into)]
#[deref(forward)]
#[as_ref(forward)]
pub struct AbsPathBuf(PathBuf);

impl AbsPathBuf {
    /// Wrap `path` if it is absolute, otherwise return `None`.
    pub fn new(path: PathBuf) -> Option<Self> {
        if path.is_absolute() {
            Some(Self(path))
        } else {
            None
        }
    }

    /// Join a path component onto this absolute path.
    ///
    /// The result is always absolute: if `tail` is absolute it replaces `self`
    /// (same as [`Path::join`]); otherwise it is appended.
    ///
    /// This inherent method shadows [`Path::join`] from the [`Deref`] target so
    /// the absolute invariant is preserved without re-wrapping.
    #[must_use]
    pub fn join<P: AsRef<Path>>(&self, tail: P) -> AbsPathBuf {
        AbsPathBuf(self.0.join(tail))
    }

    /// Borrow as an [`AbsPath`].
    #[must_use]
    pub fn as_abs_path(&self) -> AbsPath<'_> {
        AbsPath(self.0.as_path())
    }

    /// Consume and return the inner [`PathBuf`].
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl Borrow<Path> for AbsPathBuf {
    fn borrow(&self) -> &Path {
        self.0.as_path()
    }
}

impl fmt::Display for AbsPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.display(), f)
    }
}
