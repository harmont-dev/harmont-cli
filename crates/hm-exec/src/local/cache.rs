//! Host-side cache decision.
//!
//! Resolves a wire-typed [`CommandStep`] against Docker image tags
//! to decide whether to skip execution (cache hit) or run + commit.
//!
//! Cache keys are computed by `harmont.keygen` at plan time and ride
//! along the JSON in `cache.key`.

use hm_plugin_protocol::CommandStep;

use super::docker_client::DockerClient;

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

/// Derive a deterministic Docker image tag for a cacheable step.
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

/// Remove Docker images for `step_key` that don't match `current_tag`.
pub(crate) async fn evict_stale_docker_tags(
    docker: &DockerClient,
    step_key: &str,
    current_tag: Option<&str>,
) {
    let safe = sanitize_for_tag(step_key);
    let reference = format!("harmont-cache/{safe}");
    let tags = match docker.list_images_by_reference(&reference).await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(%e, "failed to list images for stale eviction");
            return;
        }
    };
    for tag in tags {
        if Some(tag.as_str()) == current_tag {
            continue;
        }
        if let Err(e) = docker.remove_image(&tag).await {
            tracing::warn!(image = %tag, %e, "failed to remove stale cached Docker image");
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use hm_plugin_protocol::Cache;

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

    #[test]
    fn sanitize_replaces_invalid_chars() {
        assert_eq!(sanitize_for_tag("my/step.name:v1"), "my-step-name-v1");
        assert_eq!(sanitize_for_tag("simple"), "simple");
        assert_eq!(sanitize_for_tag("a_b-c"), "a_b-c");
    }

    #[test]
    fn stable_cache_tag_for_cacheable_step() {
        let s = step(Some(Cache {
            policy: "ttl".into(),
            key: Some("0123456789abcdef0000".into()),
        }));
        let tag = stable_cache_tag(&s);
        assert_eq!(
            tag,
            Some("harmont-cache/build:0123456789abcdef".to_string())
        );
    }

    #[test]
    fn stable_cache_tag_none_for_uncacheable() {
        let s = step(None);
        assert_eq!(stable_cache_tag(&s), None);
    }

    #[test]
    fn stable_cache_tag_none_for_policy_none() {
        let s = step(Some(Cache {
            policy: "none".into(),
            key: Some("abc".into()),
        }));
        assert_eq!(stable_cache_tag(&s), None);
    }
}
