//! Raw platform directory primitives.
//!
//! This module is `pub(crate)` — external callers must use
//! [`crate::dirs`] which provides Harmont-specific accessors.
//!
//! On non-Windows we intentionally hardcode `~/.config` and `~/.cache` rather
//! than reading `$XDG_CONFIG_HOME` / `$XDG_CACHE_HOME`. This keeps both
//! primitives consistent and our paths predictable; it is deliberate, not an
//! oversight. Revisit only if honoring the XDG env vars becomes a real need.

use crate::path::AbsPathBuf;

pub(crate) fn home_dir() -> Option<AbsPathBuf> {
    dirs::home_dir().and_then(AbsPathBuf::new)
}

pub(crate) fn config_dir() -> Option<AbsPathBuf> {
    if cfg!(windows) {
        dirs::config_dir().and_then(AbsPathBuf::new)
    } else {
        Some(home_dir()?.join(".config"))
    }
}

pub(crate) fn cache_dir() -> Option<AbsPathBuf> {
    if cfg!(windows) {
        dirs::cache_dir().and_then(AbsPathBuf::new)
    } else {
        Some(home_dir()?.join(".cache"))
    }
}
