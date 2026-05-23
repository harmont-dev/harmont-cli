//! Resolve which Docker image a step should boot from.

use hm_plugin_protocol::{CommandStep, SnapshotRef};

/// Pick the base image for a step at boot time.
///
/// Priority (high → low):
/// 1. Cache `hit_tag` — the host already located a satisfying
///    snapshot; boot from it.
/// 2. `parent_snapshot` — the previous step in this chain (or the
///    fork parent) committed a snapshot; chain-lineage requires we
///    boot from it so filesystem mutations propagate downstream.
/// 3. The step's `image` field.
/// 4. Fall back to `"alpine:latest"`. Root steps that want a
///    different default are tagged with `step.image = default_image`
///    by the host before dispatch (see
///    `orchestrator::graph::Graph::build`), so the plugin only
///    reaches the alpine fallback when both the pipeline and step
///    omit an image — a misconfiguration we surface rather than
///    paper over.
#[must_use]
pub(crate) fn resolve_image(
    step: &CommandStep,
    hit_tag: Option<&SnapshotRef>,
    parent_snapshot: Option<&SnapshotRef>,
) -> String {
    if let Some(tag) = hit_tag {
        return tag.to_string();
    }
    if let Some(snap) = parent_snapshot {
        return snap.to_string();
    }
    if let Some(image) = &step.image {
        return image.clone();
    }
    "alpine:latest".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step_with_image(image: Option<&str>) -> CommandStep {
        CommandStep {
            key: "k".into(),
            label: None,
            cmd: "true".into(),
            builds_in: None,
            image: image.map(String::from),
            env: None,
            timeout_seconds: None,
            cache: None,
            runner: None,
            runner_args: None,
        }
    }

    #[test]
    fn hit_tag_wins() {
        let s = step_with_image(Some("rust:1.82"));
        let hit = SnapshotRef("cache:tag".into());
        let parent = SnapshotRef("parent:tag".into());
        assert_eq!(resolve_image(&s, Some(&hit), Some(&parent)), "cache:tag");
    }

    #[test]
    fn parent_snapshot_beats_step_image() {
        let s = step_with_image(Some("rust:1.82"));
        let parent = SnapshotRef("parent:tag".into());
        assert_eq!(resolve_image(&s, None, Some(&parent)), "parent:tag");
    }

    #[test]
    fn step_image_otherwise() {
        let s = step_with_image(Some("rust:1.82"));
        assert_eq!(resolve_image(&s, None, None), "rust:1.82");
    }

    #[test]
    fn fallback_alpine_when_unset() {
        let s = step_with_image(None);
        assert_eq!(resolve_image(&s, None, None), "alpine:latest");
    }
}
