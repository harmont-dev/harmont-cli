//! Inputs to a backend run: a typed [`Plan`], source location, env, options.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use hm_pipeline_ir::PipelineGraph;
use hm_plugin_protocol::events::PlanSummary;

/// A rendered, ready-to-run pipeline.
///
/// Carries both the typed graph (for client-scheduling backends like local) and
/// the verbatim IR JSON (for forwarding backends like cloud — the server must
/// receive exactly what the DSL emitted). Parsed once, before any backend is
/// touched.
#[derive(Debug, Clone)]
pub struct Plan {
    pub graph: PipelineGraph,
    pub ir_json: String,
    pub summary: PlanSummary,
}

impl Plan {
    /// Parse verbatim IR JSON into a typed plan, retaining the original string.
    ///
    /// # Errors
    /// Returns [`crate::BackendError::Rejected`] when `ir_json` is not valid
    /// pipeline IR JSON.
    pub fn parse(ir_json: String) -> crate::Result<Self> {
        let graph: PipelineGraph = serde_json::from_slice(ir_json.as_bytes()).map_err(|e| {
            crate::BackendError::Rejected {
                code: "invalid_ir".into(),
                message: format!("could not parse pipeline IR: {e}"),
            }
        })?;
        let summary = summarize(&graph);
        Ok(Self {
            graph,
            ir_json,
            summary,
        })
    }
}

/// Build a [`PlanSummary`] from a parsed graph.
///
/// - `step_count` = number of nodes.
/// - `chain_count` = number of linear `BuildsIn` chains, delegated to the
///   authoritative implementation in `local::scheduler::chain_count`.
/// - `default_runner` = `"docker"` (matches the scheduler's
///   `unwrap_or("docker")` fallback).
fn summarize(graph: &PipelineGraph) -> PlanSummary {
    PlanSummary {
        step_count: graph.node_count(),
        chain_count: crate::local::chain_count(graph.dag()),
        default_runner: "docker".to_string(),
    }
}

/// Git metadata for the worktree being submitted.
#[derive(Debug, Clone)]
pub struct SourceMeta {
    pub branch: String,
    pub commit: String,
    pub message: Option<String>,
    /// `owner/repo` from the worktree's git remote, when one exists. `None` for
    /// a remoteless worktree; the cloud backend requires it to resolve the
    /// pipeline and errors clearly when it is absent.
    pub repo_name: Option<String>,
}

/// Per-run execution options threaded through from the CLI flags.
#[derive(Debug, Clone, Default)]
pub struct RunOptions {
    pub no_cache: bool,
    pub timeout: Option<Duration>,
    /// `false` == cloud `--no-watch` (submit, emit `BuildAccepted`, return).
    pub watch: bool,
    /// When `true`, step failures do not cancel the entire build.
    /// Direct dependents are still skipped, but independent branches
    /// continue running.
    pub keep_going: bool,
}

/// All inputs needed to start a build on any [`crate::ExecutionBackend`].
#[derive(Debug, Clone)]
pub struct RunRequest {
    pub plan: Plan,
    pub repo_root: PathBuf,
    pub pipeline_slug: String,
    pub env: BTreeMap<String, String>,
    pub source: SourceMeta,
    pub options: RunOptions,
    /// When `Some`, the cloud backend submits the build directly to this
    /// already-resolved org-global pipeline slug (via `submit_build`) instead
    /// of resolving by repo identity (`submit_repo_build`). Set by the `hm run`
    /// driver after it has created or looked up the pipeline for a repo the
    /// server hasn't "discovered" (connected/pushed). `None` for the normal
    /// repo-identity path. Ignored by non-cloud backends.
    pub cloud_pipeline_slug: Option<String>,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Minimal valid empty `PipelineGraph` serialized to JSON.
    ///
    /// `PipelineGraph` uses petgraph-serde for its `graph` field; the
    /// wire shape is `{nodes, node_holes, edge_property, edges}`.
    /// The outer wrapper adds `version` (defaulted to `"0"`) and an
    /// optional `default_image`.  `"steps"` is NOT a valid field.
    const EMPTY_IR: &str = r#"{"version":"0","graph":{"nodes":[],"node_holes":[],"edge_property":"directed","edges":[]}}"#;

    #[test]
    fn plan_keeps_verbatim_json_and_typed_graph() {
        let json = EMPTY_IR.to_string();
        let plan = Plan::parse(json.clone()).expect("parse");
        assert_eq!(plan.ir_json, json); // verbatim, byte-for-byte
        assert_eq!(plan.summary.step_count, 0); // derived from the graph
    }

    #[test]
    fn plan_summary_matches_scheduler_for_single_chain() {
        // A graph with two nodes connected by a single BuildsIn edge forms one chain.
        let json = r#"{
            "version": "0",
            "default_image": "ubuntu:24.04",
            "graph": {
                "nodes": [
                    {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}},
                    {"step": {"key": "b", "cmd": "echo b"}, "env": {}}
                ],
                "node_holes": [],
                "edge_property": "directed",
                "edges": [[0, 1, "builds_in"]]
            }
        }"#
        .to_string();

        let plan = Plan::parse(json.clone()).expect("parse");
        assert_eq!(plan.summary.step_count, 2);
        assert_eq!(plan.summary.chain_count, 1);
        assert_eq!(plan.summary.default_runner, "docker");
        // ir_json is verbatim
        assert_eq!(plan.ir_json, json);
    }

    #[test]
    fn plan_summary_counts_two_independent_chains() {
        // Two root nodes with no edges → two separate chains.
        let json = r#"{
            "version": "0",
            "graph": {
                "nodes": [
                    {"step": {"key": "a", "cmd": "echo a", "image": "ubuntu:24.04"}, "env": {}},
                    {"step": {"key": "b", "cmd": "echo b", "image": "ubuntu:24.04"}, "env": {}}
                ],
                "node_holes": [],
                "edge_property": "directed",
                "edges": []
            }
        }"#
        .to_string();

        let plan = Plan::parse(json).expect("parse");
        assert_eq!(plan.summary.step_count, 2);
        assert_eq!(plan.summary.chain_count, 2);
    }

    #[test]
    fn invalid_ir_returns_rejected_error() {
        let err = Plan::parse("not json at all".to_string()).unwrap_err();
        assert!(matches!(err, crate::BackendError::Rejected { .. }));
        let msg = err.to_string();
        assert!(msg.contains("invalid_ir"));
    }

    #[test]
    fn run_options_default_is_zero() {
        let opts = RunOptions::default();
        assert!(!opts.no_cache);
        assert!(opts.timeout.is_none());
        assert!(!opts.watch);
    }
}
