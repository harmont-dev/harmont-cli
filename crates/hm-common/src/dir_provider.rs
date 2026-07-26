//! Platform user directories, resolved once and read as borrowed paths.
//!
//! [`DirProvider`] resolves the operating system's per-user directory roots a
//! single time at construction and hands them out as `&Path`, so consumers
//! neither re-resolve the platform roots per call nor allocate to read one. It
//! is deliberately application-agnostic: it knows nothing of Harmont's own
//! `hm/` subdirectory — callers join that (and any file name) onto these roots.
//!
//! On non-Windows we intentionally hardcode `~/.config` and `~/.cache` rather
//! than honoring `$XDG_CONFIG_HOME` / `$XDG_CACHE_HOME`: it keeps our paths
//! predictable and the two roots consistent. Revisit only if honoring the XDG
//! env vars becomes a real need.

use std::path::{Path, PathBuf};

/// The platform per-user directory roots, resolved once at construction.
///
/// Build with [`DirProvider::new`], then read the `&Path` accessors. Attach a
/// process-wide instance to the app runtime and reach it via its `dirs()`
/// accessor instead of reconstructing one per call.
#[derive(Debug, Clone)]
pub struct DirProvider {
    config: PathBuf,
    cache: PathBuf,
}

impl DirProvider {
    /// Resolve the platform directory roots, once.
    ///
    /// On non-Windows the roots are `~/.config` and `~/.cache` (the `$XDG_*`
    /// env vars are intentionally ignored). Returns `None` if a root cannot be
    /// determined — e.g. there is no home directory.
    #[must_use]
    pub fn new() -> Option<Self> {
        #[cfg(windows)]
        let (config, cache) = (dirs::config_dir()?, dirs::cache_dir()?);
        #[cfg(not(windows))]
        let (config, cache) = {
            let home = dirs::home_dir()?;
            (home.join(".config"), home.join(".cache"))
        };

        Some(Self { config, cache })
    }

    /// The user configuration root (`~/.config` on non-Windows).
    #[must_use]
    pub fn config(&self) -> &Path {
        &self.config
    }

    /// The user cache root (`~/.cache` on non-Windows).
    #[must_use]
    pub fn cache(&self) -> &Path {
        &self.cache
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn config_is_the_platform_config_root() {
        let dirs = DirProvider::new().unwrap();
        assert!(dirs.config().is_absolute(), "got {:?}", dirs.config());
        #[cfg(not(windows))]
        assert!(dirs.config().ends_with(".config"), "got {:?}", dirs.config());
    }

    #[rstest]
    fn cache_is_the_platform_cache_root() {
        let dirs = DirProvider::new().unwrap();
        assert!(dirs.cache().is_absolute(), "got {:?}", dirs.cache());
        #[cfg(not(windows))]
        assert!(dirs.cache().ends_with(".cache"), "got {:?}", dirs.cache());
    }
}
