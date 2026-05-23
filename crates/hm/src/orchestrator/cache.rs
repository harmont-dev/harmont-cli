//! Host-side cache decision.
//!
//! Resolves a wire-typed [`CommandStep`] against the local Docker
//! daemon and returns the wire-typed [`CacheDecision`] consumed by
//! step-executor plugins (design spec §5.5).
//!
//! Cache keys are computed by `harmont.keygen` at plan time and ride
//! along the JSON in `cache.key`. We turn them into Docker image tags
//! and consult the local image store.

use anyhow::Result;
use hm_plugin_protocol::{CacheDecision, CommandStep, SnapshotRef};

use crate::orchestrator::docker_client::DockerClient;

/// `harmont-local/<step_key>:<cache_key_first_16_hex>`. Step key is
/// sanitised to `[a-zA-Z0-9_-]` (Docker tag rules). Returns `None`
/// when the step has no cache or a policy of `"none"`.
///
/// The cache key is the SHA-256 hex resolved at plan time by
/// `harmont.keygen`. We truncate to the first 16 hex chars (8 bytes)
/// for the image tag — collision odds across a developer's local
/// cache are negligible. The cloud path uses the full key elsewhere;
/// that divergence is acceptable for local-only tags since they're
/// never resolved across machines.
fn cache_image_tag(step: &CommandStep) -> Option<String> {
    let cache = step.cache.as_ref()?;
    if cache.policy == "none" {
        return None;
    }
    let key = cache.key.as_deref()?;
    let safe = sanitize_for_tag(&step.key);
    let short = &key[..key.len().min(16)];
    Some(format!("harmont-local/{safe}:{short}"))
}

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

/// Decide cache outcome for a step against the local Docker daemon.
///
/// Returns hit (snapshot already present), miss-with-tag (run and commit
/// afterwards), or miss-no-commit (`cache.policy == "none"` or no cache
/// key).
///
/// # Errors
/// Returns an error if the Docker daemon `image_exists` call fails.
pub async fn decide(docker: &DockerClient, step: &CommandStep) -> Result<CacheDecision> {
    let Some(tag) = cache_image_tag(step) else {
        return Ok(CacheDecision::MissNoCommit);
    };
    if docker.image_exists(&tag).await? {
        Ok(CacheDecision::Hit {
            tag: SnapshotRef::from(tag),
        })
    } else {
        Ok(CacheDecision::MissBuildAs {
            tag: SnapshotRef::from(tag),
        })
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
            builds_in: None,
            image: None,
            env: None,
            timeout_seconds: None,
            cache,
            runner: None,
            runner_args: None,
        }
    }

    #[test]
    fn no_cache_yields_none() {
        assert!(cache_image_tag(&step(None)).is_none());
    }

    #[test]
    fn policy_none_yields_none() {
        let s = step(Some(Cache {
            policy: "none".into(),
            key: Some("abcdef".into()),
        }));
        assert!(cache_image_tag(&s).is_none());
    }

    #[test]
    fn ttl_with_key_yields_tag() {
        let s = step(Some(Cache {
            policy: "ttl".into(),
            key: Some("0123456789abcdefffff".into()),
        }));
        let tag = cache_image_tag(&s).unwrap();
        assert!(tag.starts_with("harmont-local/build:"));
    }
}
