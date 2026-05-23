//! Raw platform directory primitives.
//!
//! This module is `pub(crate)` — external callers must use
//! [`crate::dirs`] which provides Harmont-specific accessors.

use std::path::PathBuf;

pub(crate) fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    dirs::config_dir()
}
