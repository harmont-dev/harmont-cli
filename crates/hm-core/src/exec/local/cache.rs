//! Host-side cache key derivation.
//!
//! Resolves a wire-typed [`CommandStep`] to a deterministic cache key
//! so the scheduler can pass it to the runner for hit/miss decisions.
//!
//! Cache keys are computed by `harmont.keygen` at plan time and ride
//! along the JSON in `cache.key`.

use hm_plugin_protocol::CommandStep;

fn sanitize_for_tag(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Derive a deterministic cache tag for a cacheable step.
///
/// Returns `None` when the step has no cache, a `"none"` policy, or no
/// cache key.
#[must_use]
pub(crate) fn stable_cache_tag(step: &CommandStep) -> Option<String> {
    let cache = step.cache.as_ref()?;
    if cache.policy == "none" {
        return None;
    }
    let key = cache.key.as_deref()?;
    let safe = sanitize_for_tag(&step.key);
    let short = &key[..key.len().min(16)];
    Some(format!("harmont-cache/{safe}:{short}"))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test setup and assertions"
)]
mod tests {
    use super::*;
    use hm_plugin_protocol::Cache;
    use rstest::rstest;

    fn step(cache: Option<Cache>) -> CommandStep {
        CommandStep {
            key: "build".into(),
            label: None,
            cmd: "true".into(),
            image: None,
            env: None,
            timeout_seconds: None,
            cache,
            runner: None,
            runner_args: None,
        }
    }

    #[rstest]
    #[case::mixed("my/step.name:v1", "my-step-name-v1")]
    #[case::simple("simple", "simple")]
    #[case::already_safe("a_b-c", "a_b-c")]
    fn sanitize_replaces_invalid_chars(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(sanitize_for_tag(input), expected);
    }

    #[rstest]
    #[case::cacheable(
        Some(Cache { policy: "ttl".into(), key: Some("0123456789abcdef0000".into()) }),
        Some("harmont-cache/build:0123456789abcdef".to_string())
    )]
    #[case::uncacheable(None, None)]
    #[case::policy_none(
        Some(Cache { policy: "none".into(), key: Some("abc".into()) }),
        None
    )]
    fn stable_cache_tag_derivation(#[case] cache: Option<Cache>, #[case] expected: Option<String>) {
        let s = step(cache);
        assert_eq!(stable_cache_tag(&s), expected);
    }
}
