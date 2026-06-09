//! Raw platform directory primitives.
//!
//! This module is `pub(crate)` — external callers must use
//! [`crate::dirs`] which provides Harmont-specific accessors.
//!
//! On non-Windows we intentionally hardcode `~/.config` and `~/.cache` rather
//! than reading `$XDG_CONFIG_HOME` / `$XDG_CACHE_HOME`. This keeps both
//! primitives consistent and our paths predictable; it is deliberate, not an
//! oversight. Revisit only if honoring the XDG env vars becomes a real need.

use std::path::PathBuf;

pub(crate) fn home_dir() -> Option<PathBuf> {
    dirs::home_dir()
}

pub(crate) fn config_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::config_dir()
    } else {
        home_dir().map(|h| h.join(".config"))
    }
}

pub(crate) fn cache_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        dirs::cache_dir()
    } else {
        home_dir().map(|h| h.join(".cache"))
    }
}
