//! Host-side cache decision.
//!
//! Resolves a wire-typed [`CommandStep`] against the local Docker
//! daemon and returns the wire-typed [`CacheDecision`] consumed by
//! step-executor plugins (design spec §5.5).
//!
//! Cache keys are computed by `harmont.keygen` at plan time and ride
//! along the JSON in `cache.key`. We turn them into Docker image tags
//! and consult the local image store.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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

/// The outcome of a cache lookup: the wire-typed decision plus any
/// stale images that should be garbage-collected after the new image
/// is committed.
#[derive(Debug)]
pub struct CacheOutcome {
    pub decision: CacheDecision,
    /// Stale cache images for this step that should be removed after
    /// the new image is committed successfully.
    pub stale_tags: Vec<String>,
}

/// Decide cache outcome for a step against the local Docker daemon.
///
/// Returns hit (snapshot already present), miss-with-tag (run and commit
/// afterwards), or miss-no-commit (`cache.policy == "none"` or no cache
/// key).
///
/// # Errors
/// Returns an error if the Docker daemon `image_exists` call fails.
pub async fn decide(docker: &DockerClient, step: &CommandStep) -> Result<CacheOutcome> {
    let Some(tag) = cache_image_tag(step) else {
        return Ok(CacheOutcome {
            decision: CacheDecision::MissNoCommit,
            stale_tags: vec![],
        });
    };
    if docker.image_exists(&tag).await? {
        Ok(CacheOutcome {
            decision: CacheDecision::Hit {
                tag: SnapshotRef::from(tag),
            },
            stale_tags: vec![],
        })
    } else {
        let safe = sanitize_for_tag(&step.key);
        let prefix = format!("harmont-local/{safe}");
        let stale = docker
            .list_images_by_reference(&prefix)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|t| *t != tag)
            .collect();
        Ok(CacheOutcome {
            decision: CacheDecision::MissBuildAs {
                tag: SnapshotRef::from(tag),
            },
            stale_tags: stale,
        })
    }
}

// ---------------------------------------------------------------------------
// COW workspace cache
// ---------------------------------------------------------------------------

/// The outcome of a COW workspace cache lookup.
#[derive(Debug)]
pub struct CowCacheOutcome {
    pub decision: CacheDecision,
    pub cache_to: Option<PathBuf>,
    pub stale_dirs: Vec<PathBuf>,
}

/// Resolve the on-disk cache directory for a step's COW workspace.
///
/// Returns `None` when the step has no cache, a `"none"` policy, or no
/// cache key — matching the same guard logic as [`cache_image_tag`].
pub fn cow_cache_dir(step: &CommandStep) -> Result<Option<PathBuf>> {
    let cache = match step.cache.as_ref() {
        Some(c) if c.policy != "none" => c,
        _ => return Ok(None),
    };
    let key = match cache.key.as_deref() {
        Some(k) => k,
        None => return Ok(None),
    };
    let base = hm_util::dirs::harmont_config_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot resolve ~/.harmont"))?;
    let safe = sanitize_for_tag(&step.key);
    let short = &key[..key.len().min(16)];
    Ok(Some(
        base.join("cache")
            .join("workspaces")
            .join(safe)
            .join(short),
    ))
}

/// Decide cache outcome for a step against the local COW workspace
/// cache directory.
///
/// # Errors
/// Returns an error if the config directory cannot be resolved or the
/// stale directory listing fails.
pub fn decide_cow(step: &CommandStep) -> Result<CowCacheOutcome> {
    let Some(cache_dir) = cow_cache_dir(step)? else {
        return Ok(CowCacheOutcome {
            decision: CacheDecision::MissNoCommit,
            cache_to: None,
            stale_dirs: vec![],
        });
    };
    if cache_dir.exists() {
        Ok(CowCacheOutcome {
            decision: CacheDecision::Hit {
                tag: SnapshotRef::from(format!("cow:{}", cache_dir.display())),
            },
            cache_to: None,
            stale_dirs: vec![],
        })
    } else {
        let step_cache_root = cache_dir.parent().expect("cache_dir always has parent");
        let stale = if step_cache_root.exists() {
            std::fs::read_dir(step_cache_root)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| *p != cache_dir)
                .collect()
        } else {
            vec![]
        };
        Ok(CowCacheOutcome {
            decision: CacheDecision::MissBuildAs {
                tag: SnapshotRef::from(format!("cow:{}", cache_dir.display())),
            },
            cache_to: Some(cache_dir),
            stale_dirs: stale,
        })
    }
}

/// Persist a completed workspace directory into the COW cache.
///
/// Creates intermediate directories and performs a COW clone. If the
/// cache directory already exists (e.g. a concurrent run beat us) the
/// function returns `Ok(())` without overwriting.
///
/// # Errors
/// Returns an error if the parent directory cannot be created or the
/// COW clone fails.
pub fn persist_cow_cache(workspace_path: &Path, cache_dir: &Path) -> Result<()> {
    if let Some(parent) = cache_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if cache_dir.exists() {
        return Ok(());
    }
    hm_util::cow::cow_clone_dir(workspace_path, cache_dir)
        .context("persist workspace to COW cache")
}

/// Remove stale COW cache directories left over from previous cache
/// keys. Failures are logged but never propagated.
pub fn evict_stale_cow_dirs(dirs: &[PathBuf]) {
    for dir in dirs {
        if let Err(e) = std::fs::remove_dir_all(dir) {
            tracing::warn!(path = %dir.display(), %e, "failed to evict stale COW cache");
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

    #[test]
    fn sanitize_replaces_invalid_chars() {
        assert_eq!(sanitize_for_tag("my/step.name:v1"), "my-step-name-v1");
        assert_eq!(sanitize_for_tag("simple"), "simple");
        assert_eq!(sanitize_for_tag("a_b-c"), "a_b-c");
    }

    // ------------------------------------------------------------------
    // COW workspace cache tests
    // ------------------------------------------------------------------

    #[test]
    fn cow_cache_dir_returns_path_for_cacheable_step() {
        let s = step(Some(Cache {
            policy: "ttl".into(),
            key: Some("0123456789abcdef0000".into()),
        }));
        let dir = cow_cache_dir(&s).unwrap();
        assert!(dir.is_some(), "expected Some for cacheable step");
        let dir = dir.unwrap();
        assert!(
            dir.ends_with("cache/workspaces/build/0123456789abcdef"),
            "unexpected path: {}",
            dir.display()
        );
    }

    #[test]
    fn cow_cache_dir_returns_none_for_no_cache() {
        let s = step(None);
        let dir = cow_cache_dir(&s).unwrap();
        assert!(dir.is_none());
    }

    #[test]
    fn cow_cache_dir_returns_none_for_policy_none() {
        let s = step(Some(Cache {
            policy: "none".into(),
            key: Some("abcdef1234567890".into()),
        }));
        let dir = cow_cache_dir(&s).unwrap();
        assert!(dir.is_none());
    }

    #[test]
    fn decide_cow_miss_no_commit_when_no_cache() {
        let s = step(None);
        let outcome = decide_cow(&s).unwrap();
        assert!(outcome.decision.is_miss_no_commit());
        assert!(outcome.cache_to.is_none());
        assert!(outcome.stale_dirs.is_empty());
    }

    #[test]
    fn decide_cow_miss_build_as_for_new_key() {
        // Use a unique key that will not exist on disk.
        let s = step(Some(Cache {
            policy: "ttl".into(),
            key: Some("deadbeefcafebabe9999".into()),
        }));
        let outcome = decide_cow(&s).unwrap();
        assert!(
            outcome.decision.is_miss_build_as(),
            "expected MissBuildAs, got {:?}",
            outcome.decision
        );
        assert!(outcome.cache_to.is_some());
    }
}
