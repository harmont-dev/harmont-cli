use std::collections::BTreeMap;
use std::num::NonZeroU32;

use daggy::Dag;

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

/// A single build command within a pipeline.
///
/// Serialized as a JSON object inside each graph node's `step` field.
/// The `key` is the unique identifier used to reference this step in
/// edges and log output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct CommandStep {
    /// Unique identifier for this step within the pipeline.
    pub key: String,
    /// Human-readable label shown in build output.
    #[serde(default)]
    pub label: Option<String>,
    /// Shell command to execute inside the container.
    pub cmd: String,
    /// Docker image to boot from. Root steps without an image inherit
    /// `PipelineGraph::default_image`; child steps boot from their
    /// parent's committed snapshot.
    #[serde(default)]
    pub image: Option<String>,
    /// Per-step environment variables merged on top of the pipeline env.
    #[serde(default)]
    pub env: Option<BTreeMap<String, String>>,
    /// Maximum wall-clock seconds before the step is killed.
    ///
    /// `NonZeroU32`: a `0`-second budget is rejected at the wire boundary.
    #[serde(default)]
    pub timeout_seconds: Option<NonZeroU32>,
    /// Cache configuration for this step's committed snapshot.
    #[serde(default)]
    pub cache: Option<Cache>,
    /// Step-executor plugin name. `None` falls back to the default
    /// runner (Docker in the shipped configuration).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner: Option<String>,
    /// Plugin-specific extra fields passed verbatim to the runner.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_args: Option<serde_json::Value>,
}

/// Snapshot cache configuration for a step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, DeriveJsonSchema)]
pub struct Cache {
    /// Cache policy name (e.g. `"content-hash"`).
    pub policy: String,
    /// Explicit cache key override; derived from the step if absent.
    #[serde(default)]
    pub key: Option<String>,
}

/// A graph node: a [`CommandStep`] paired with its resolved environment.
///
/// The `env` map is the final merged result of pipeline-level defaults
/// and per-step overrides — ready to hand to the executor as-is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub step: CommandStep,
    pub env: BTreeMap<String, String>,
}

/// Edge label in the pipeline DAG.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EdgeKind {
    /// Container lineage: the child boots from the parent's committed
    /// snapshot rather than from a fresh image.
    BuildsIn,
    /// Ordering-only dependency (emitted by `wait` barriers). The
    /// child waits for the parent to finish but does not inherit its
    /// snapshot.
    DependsOn,
}

/// Top-level pipeline graph, deserialized directly from the v0 wire
/// format (petgraph-serde JSON).
///
/// Callers access the underlying [`Dag`] via [`dag()`](Self::dag) and
/// traverse it with petgraph's standard visitor traits.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineGraph {
    #[serde(default = "default_version")]
    version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    default_image: Option<String>,
    /// Whole-build wall-clock budget in seconds. When set, the local
    /// orchestrator kills the run and fails it once this elapses.
    ///
    /// `NonZeroU32` makes a `0`-second budget (kill immediately) an
    /// unrepresentable, wire-rejected value rather than a runtime footgun.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    timeout_seconds: Option<NonZeroU32>,
    #[serde(rename = "graph")]
    inner: Dag<Transition, EdgeKind>,
}

fn default_version() -> String {
    "0".to_string()
}

impl PipelineGraph {
    /// Number of steps (nodes) in the graph.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.inner.node_count()
    }

    /// Pipeline-wide fallback image for root steps that don't declare one.
    #[must_use]
    pub fn default_image(&self) -> Option<&str> {
        self.default_image.as_deref()
    }

    /// Whole-build wall-clock budget in seconds, if the author set one.
    ///
    /// The returned value is positive by construction (`0` is rejected at
    /// the wire boundary), so consumers need no `> 0` guard.
    #[must_use]
    pub const fn timeout_seconds(&self) -> Option<NonZeroU32> {
        self.timeout_seconds
    }

    /// The underlying DAG for direct traversal.
    #[must_use]
    pub const fn dag(&self) -> &Dag<Transition, EdgeKind> {
        &self.inner
    }
}

#[cfg(test)]
mod timeout_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use std::num::NonZeroU32;

    use super::PipelineGraph;

    #[test]
    fn deserializes_pipeline_timeout_seconds() {
        let json = r#"{
            "version": "0",
            "timeout_seconds": 1800,
            "graph": {"nodes": [], "node_holes": [], "edge_property": "directed", "edges": []}
        }"#;
        let g: PipelineGraph = serde_json::from_str(json).unwrap();
        assert_eq!(g.timeout_seconds(), NonZeroU32::new(1800));
    }

    #[test]
    fn rejects_zero_pipeline_timeout_seconds() {
        let json = r#"{
            "version": "0",
            "timeout_seconds": 0,
            "graph": {"nodes": [], "node_holes": [], "edge_property": "directed", "edges": []}
        }"#;
        assert!(serde_json::from_str::<PipelineGraph>(json).is_err());
    }

    #[test]
    fn pipeline_timeout_defaults_to_none() {
        let json = r#"{
            "version": "0",
            "graph": {"nodes": [], "node_holes": [], "edge_property": "directed", "edges": []}
        }"#;
        let g: PipelineGraph = serde_json::from_str(json).unwrap();
        assert_eq!(g.timeout_seconds(), None);
    }
}
