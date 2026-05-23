//! Raw platform directory primitives.
//!
//! This module is `pub(crate)` — external callers must use
//! [`crate::dirs`] which provides Harmont-specific accessors.

#![allow(unreachable_pub)]

use std::path::PathBuf;

pub fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

pub fn config_dir() -> Option<PathBuf> {
    dirs::config_dir()
}
