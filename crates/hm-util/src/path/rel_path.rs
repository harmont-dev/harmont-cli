//! A path that is guaranteed to be relative.
//!
//! [`RelPath`] / [`RelPathBuf`] mirror the [`Path`] / [`PathBuf`] split from
//! `std`, adding the invariant that the path is relative.

use std::borrow::Borrow;
use std::ffi::OsStr;
use std::fmt;
use std::path::{Components, Path, PathBuf};

/// A borrowed path that is guaranteed to be relative.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(derive_more::AsRef)]
#[as_ref(forward)]
pub struct RelPath<'a>(&'a Path);

impl<'a> RelPath<'a> {
    /// Wrap `path` if it is relative, otherwise return `None`.
    pub fn new<P: AsRef<Path> + ?Sized>(path: &'a P) -> Option<Self> {
        let p = path.as_ref();
        if p.is_relative() {
            Some(Self(p))
        } else {
            None
        }
    }

    /// Append another path segment.
    #[must_use]
    pub fn join<P: AsRef<Path>>(&self, tail: P) -> PathBuf {
        self.0.join(tail)
    }

    /// The final component of this path, if any.
    #[must_use]
    pub fn file_name(&self) -> Option<&OsStr> {
        self.0.file_name()
    }

    /// An iterator over the components of this path.
    pub fn components(&self) -> Components<'_> {
        self.0.components()
    }

    /// Yield the underlying [`Path`].
    #[must_use]
    pub const fn as_path(&self) -> &Path {
        self.0
    }

    /// Convert to an owned [`RelPathBuf`].
    #[must_use]
    pub fn to_rel_path_buf(&self) -> RelPathBuf {
        RelPathBuf(self.0.to_path_buf())
    }
}

impl fmt::Display for RelPath<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.display(), f)
    }
}

impl<'a> From<&'a RelPathBuf> for RelPath<'a> {
    fn from(buf: &'a RelPathBuf) -> Self {
        buf.as_rel_path()
    }
}

/// An owned path that is guaranteed to be relative.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[derive(derive_more::Deref, derive_more::AsRef, derive_more::Into)]
#[deref(forward)]
#[as_ref(forward)]
pub struct RelPathBuf(PathBuf);

impl RelPathBuf {
    /// Wrap `path` if it is relative, otherwise return `None`.
    #[must_use]
    pub fn new(path: PathBuf) -> Option<Self> {
        if path.is_relative() {
            Some(Self(path))
        } else {
            None
        }
    }

    /// Borrow as a [`RelPath`].
    #[must_use]
    pub fn as_rel_path(&self) -> RelPath<'_> {
        RelPath(self.0.as_path())
    }

    /// Consume and return the inner [`PathBuf`].
    #[must_use]
    pub fn into_path_buf(self) -> PathBuf {
        self.0
    }
}

impl Borrow<Path> for RelPathBuf {
    fn borrow(&self) -> &Path {
        self.0.as_path()
    }
}

impl fmt::Display for RelPathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0.display(), f)
    }
}
