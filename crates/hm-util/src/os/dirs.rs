//! Raw platform directory primitives.
//!
//! These are the unopinionated platform roots — no `hm` namespacing. Where
//! *this* tool keeps *this user's* state (`~/.config/hm`, `~/.cache/hm`, …) is
//! policy, not a platform fact, and lives on `hm_core::Sys`.
//!
//! Each root is absolute by construction: they resolve from `$HOME` /
//! `%APPDATA%`, so `None` means only "the platform has no such directory".
//!
//! On non-Windows we intentionally hardcode `~/.config` and `~/.cache` rather
//! than reading `$XDG_CONFIG_HOME` / `$XDG_CACHE_HOME`. This keeps both
//! primitives consistent and our paths predictable; it is deliberate, not an
//! oversight. Revisit only if honoring the XDG env vars becomes a real need.

use crate::path::AbsPathBuf;

/// The invoking user's home directory.
#[must_use]
pub fn home_dir() -> Option<AbsPathBuf> {
    dirs::home_dir().and_then(AbsPathBuf::new)
}

/// The platform configuration root (`~/.config`, `%APPDATA%`).
#[must_use]
pub fn config_dir() -> Option<AbsPathBuf> {
    if cfg!(windows) {
        dirs::config_dir().and_then(AbsPathBuf::new)
    } else {
        home_dir().map(|h| h.join(".config"))
    }
}

/// The platform cache root (`~/.cache`, `%LOCALAPPDATA%`).
#[must_use]
pub fn cache_dir() -> Option<AbsPathBuf> {
    if cfg!(windows) {
        dirs::cache_dir().and_then(AbsPathBuf::new)
    } else {
        home_dir().map(|h| h.join(".cache"))
    }
}
