//! Runtime capability introspection. The CLI consults this BEFORE `start` to
//! validate flags (e.g. reject `--parallelism` on an observer backend) and to
//! phrase output (e.g. print a watch URL only when `provides_watch_url`).
//! `#[non_exhaustive]`: new capabilities default off; old backends stay valid.

#[derive(Debug, Clone, Copy)]
#[non_exhaustive]
// Each field is an independent yes/no capability; a state machine would obscure
// orthogonal combinations (e.g. local can honor parallelism AND enforce timeout).
#[allow(clippy::struct_excessive_bools)]
pub struct Capabilities {
    pub honors_parallelism: bool,
    pub is_observer: bool,        // submits + watches (cloud) vs executes (local)
    pub reports_cache_hits: bool,
    pub supports_no_watch: bool,
    pub provides_watch_url: bool,
    pub enforces_timeout: bool,
}

impl Capabilities {
    #[must_use]
    pub const fn none() -> Self {
        Self {
            honors_parallelism: false,
            is_observer: false,
            reports_cache_hits: false,
            supports_no_watch: false,
            provides_watch_url: false,
            enforces_timeout: false,
        }
    }

    #[must_use]
    pub const fn local() -> Self {
        Self {
            honors_parallelism: true,
            is_observer: false,
            reports_cache_hits: true,
            supports_no_watch: false,
            provides_watch_url: false,
            enforces_timeout: false, // TODO(timeout): local scheduler ignores RunOptions.timeout
        }
    }

    #[must_use]
    pub const fn cloud() -> Self {
        Self {
            honors_parallelism: false,
            is_observer: true,
            reports_cache_hits: false,
            supports_no_watch: true,
            provides_watch_url: true,
            enforces_timeout: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_differ() {
        assert!(Capabilities::local().honors_parallelism);
        assert!(!Capabilities::local().is_observer);
        assert!(Capabilities::cloud().is_observer);
        assert!(Capabilities::cloud().provides_watch_url);
        assert!(!Capabilities::cloud().honors_parallelism);
    }
}
