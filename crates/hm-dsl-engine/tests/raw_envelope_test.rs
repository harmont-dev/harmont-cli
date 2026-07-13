#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hm_dsl_engine::raw_envelope::{RawEnvelope, process_raw_envelope};
use serde_json::json;

/// A raw envelope with a single one-step pipeline.
fn single_pipeline_envelope() -> serde_json::Value {
    json!({
        "schema_version": "1",
        "pipelines": [{
            "slug": "ci",
            "name": "CI",
            "allow_manual": true,
            "triggers": [{"kind": "push", "branch": "main"}],
            "step_chain": {
                "steps": [
                    {"cmd": "make build", "parent_idx": null, "label": "build"}
                ],
                "leaf_indices": [0]
            }
        }]
    })
}

#[test]
fn deserializes_and_processes_single_pipeline() {
    let raw: RawEnvelope = serde_json::from_value(single_pipeline_envelope()).unwrap();
    assert_eq!(raw.schema_version, "1");
    assert_eq!(raw.pipelines.len(), 1);

    let final_env = process_raw_envelope(raw).unwrap();
    assert_eq!(final_env.schema_version, "1");
    assert_eq!(final_env.pipelines.len(), 1);

    let entry = &final_env.pipelines[0];
    // The final entry carries a lowered `definition`, never a `step_chain`.
    let serialized = serde_json::to_value(entry).unwrap();
    assert!(serialized.get("definition").is_some());
    assert!(serialized.get("step_chain").is_none());
}

#[test]
fn definition_is_valid_v0_ir() {
    let raw: RawEnvelope = serde_json::from_value(single_pipeline_envelope()).unwrap();
    let final_env = process_raw_envelope(raw).unwrap();
    let def = &final_env.pipelines[0].definition;

    assert_eq!(def.get("version").and_then(|v| v.as_str()), Some("0"));

    let graph = def.get("graph").expect("definition has a graph");
    let nodes = graph.get("nodes").and_then(|n| n.as_array()).unwrap();
    assert_eq!(nodes.len(), 1);
    assert!(graph.get("edges").and_then(|e| e.as_array()).unwrap().is_empty());

    let step = &nodes[0]["step"];
    assert_eq!(step["cmd"].as_str(), Some("make build"));
    assert_eq!(step["label"].as_str(), Some("build"));
    // Imageless root steps get the pipeline-wide default image.
    assert_eq!(step["image"].as_str(), Some("ubuntu:24.04"));

    // The `definition` round-trips as a real PipelineGraph.
    let parsed: hm_pipeline_ir::PipelineGraph = serde_json::from_value(def.clone()).unwrap();
    assert_eq!(parsed.node_count(), 1);
}

#[test]
fn metadata_passes_through() {
    let raw: RawEnvelope = serde_json::from_value(single_pipeline_envelope()).unwrap();
    let final_env = process_raw_envelope(raw).unwrap();
    let entry = &final_env.pipelines[0];

    assert_eq!(entry.slug, "ci");
    assert_eq!(entry.name, "CI");
    assert!(entry.allow_manual);
    assert_eq!(entry.triggers.len(), 1);
    assert_eq!(entry.triggers[0]["kind"].as_str(), Some("push"));
}

#[test]
fn processes_multiple_pipelines() {
    let raw: RawEnvelope = serde_json::from_value(json!({
        "schema_version": "1",
        "pipelines": [
            {
                "slug": "one",
                "name": "One",
                "step_chain": {
                    "steps": [{"cmd": "echo a", "parent_idx": null}],
                    "leaf_indices": [0]
                }
            },
            {
                "slug": "two",
                "name": "Two",
                "step_chain": {
                    "steps": [
                        {"cmd": "echo a", "parent_idx": null},
                        {"cmd": "echo b", "parent_idx": 0}
                    ],
                    "leaf_indices": [1]
                }
            }
        ]
    }))
    .unwrap();

    let final_env = process_raw_envelope(raw).unwrap();
    assert_eq!(final_env.pipelines.len(), 2);
    assert_eq!(final_env.pipelines[0].slug, "one");
    assert_eq!(final_env.pipelines[1].slug, "two");

    let two: hm_pipeline_ir::PipelineGraph =
        serde_json::from_value(final_env.pipelines[1].definition.clone()).unwrap();
    assert_eq!(two.node_count(), 2);
}

#[test]
fn metadata_defaults_when_omitted() {
    let raw: RawEnvelope = serde_json::from_value(json!({
        "schema_version": "1",
        "pipelines": [{
            "slug": "min",
            "name": "Minimal",
            "step_chain": {
                "steps": [{"cmd": "true", "parent_idx": null}],
                "leaf_indices": [0]
            }
        }]
    }))
    .unwrap();

    let final_env = process_raw_envelope(raw).unwrap();
    let entry = &final_env.pipelines[0];
    assert!(!entry.allow_manual);
    assert!(entry.triggers.is_empty());
}
