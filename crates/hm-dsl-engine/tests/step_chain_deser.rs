#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use hm_dsl_engine::step_chain::{RawCachePolicy, RawStepChain};

#[test]
fn minimal_scratch_plus_command() {
    let json = r#"{
        "steps": [
            {"cmd": null, "parent_idx": null},
            {"cmd": "make build", "parent_idx": 0}
        ],
        "leaf_indices": [1]
    }"#;
    let chain: RawStepChain = serde_json::from_str(json).unwrap();
    assert_eq!(chain.steps.len(), 2);
    assert_eq!(chain.leaf_indices, vec![1]);
    assert!(chain.steps[0].cmd.is_none());
    assert!(chain.steps[0].parent_idx.is_none());
    assert_eq!(chain.steps[1].cmd.as_deref(), Some("make build"));
    assert_eq!(chain.steps[1].parent_idx, Some(0));
    assert!(chain.pipeline_env.is_none());
    assert!(chain.pipeline_timeout_seconds.is_none());
}

#[test]
fn step_with_all_fields_populated() {
    let json = r#"{
        "steps": [{
            "cmd": "pytest",
            "parent_idx": 3,
            "is_wait": false,
            "continue_on_failure": true,
            "label": "run tests",
            "cache": {"policy": "none"},
            "env": {"CI": "1", "FOO": "bar"},
            "timeout_seconds": 600,
            "image": "python:3.12",
            "runner": "docker",
            "runner_args": {"privileged": true},
            "key_override": "custom-key"
        }],
        "leaf_indices": [0]
    }"#;
    let chain: RawStepChain = serde_json::from_str(json).unwrap();
    let step = &chain.steps[0];
    assert_eq!(step.cmd.as_deref(), Some("pytest"));
    assert_eq!(step.parent_idx, Some(3));
    assert!(!step.is_wait);
    assert!(step.continue_on_failure);
    assert_eq!(step.label.as_deref(), Some("run tests"));
    assert!(matches!(step.cache, Some(RawCachePolicy::None)));
    let env = step.env.as_ref().unwrap();
    assert_eq!(env.get("CI").map(String::as_str), Some("1"));
    assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
    assert_eq!(step.timeout_seconds, Some(600));
    assert_eq!(step.image.as_deref(), Some("python:3.12"));
    assert_eq!(step.runner.as_deref(), Some("docker"));
    assert_eq!(step.runner_args.as_ref().unwrap()["privileged"], true);
    assert_eq!(step.key_override.as_deref(), Some("custom-key"));
}

#[test]
fn cache_policy_none() {
    let step = single_step_with_cache(r#"{"policy": "none"}"#);
    assert!(matches!(step, RawCachePolicy::None));
}

#[test]
fn cache_policy_forever() {
    let step = single_step_with_cache(r#"{"policy": "forever", "env_keys": ["A", "B"]}"#);
    match step {
        RawCachePolicy::Forever { env_keys } => assert_eq!(env_keys, vec!["A", "B"]),
        other => panic!("expected Forever, got {other:?}"),
    }
}

#[test]
fn cache_policy_forever_defaults_env_keys() {
    let step = single_step_with_cache(r#"{"policy": "forever"}"#);
    match step {
        RawCachePolicy::Forever { env_keys } => assert!(env_keys.is_empty()),
        other => panic!("expected Forever, got {other:?}"),
    }
}

#[test]
fn cache_policy_ttl() {
    let step =
        single_step_with_cache(r#"{"policy": "ttl", "duration_seconds": 3600, "env_keys": ["X"]}"#);
    match step {
        RawCachePolicy::Ttl {
            duration_seconds,
            env_keys,
        } => {
            assert_eq!(duration_seconds, 3600);
            assert_eq!(env_keys, vec!["X"]);
        }
        other => panic!("expected Ttl, got {other:?}"),
    }
}

#[test]
fn cache_policy_on_change() {
    let step =
        single_step_with_cache(r#"{"policy": "on_change", "paths": ["src/**", "Cargo.toml"]}"#);
    match step {
        RawCachePolicy::OnChange { paths } => assert_eq!(paths, vec!["src/**", "Cargo.toml"]),
        other => panic!("expected OnChange, got {other:?}"),
    }
}

#[test]
fn cache_policy_compose() {
    let step = single_step_with_cache(
        r#"{"policy": "compose", "sub_policies": [
            {"policy": "forever"},
            {"policy": "on_change", "paths": ["a"]}
        ]}"#,
    );
    match step {
        RawCachePolicy::Compose { sub_policies } => {
            assert_eq!(sub_policies.len(), 2);
            assert!(matches!(sub_policies[0], RawCachePolicy::Forever { .. }));
            assert!(matches!(sub_policies[1], RawCachePolicy::OnChange { .. }));
        }
        other => panic!("expected Compose, got {other:?}"),
    }
}

#[test]
fn wait_step_with_continue_on_failure() {
    let json = r#"{
        "steps": [
            {"cmd": "a", "parent_idx": null},
            {"cmd": "b", "parent_idx": null},
            {"cmd": null, "parent_idx": 1, "is_wait": true, "continue_on_failure": true}
        ],
        "leaf_indices": [2]
    }"#;
    let chain: RawStepChain = serde_json::from_str(json).unwrap();
    let wait = &chain.steps[2];
    assert!(wait.is_wait);
    assert!(wait.continue_on_failure);
    assert!(wait.cmd.is_none());
}

#[test]
fn pipeline_level_env_and_timeout() {
    let json = r#"{
        "steps": [{"cmd": "build", "parent_idx": null}],
        "leaf_indices": [0],
        "pipeline_env": {"GLOBAL": "yes"},
        "pipeline_timeout_seconds": 1800
    }"#;
    let chain: RawStepChain = serde_json::from_str(json).unwrap();
    let env = chain.pipeline_env.as_ref().unwrap();
    assert_eq!(env.get("GLOBAL").map(String::as_str), Some("yes"));
    assert_eq!(chain.pipeline_timeout_seconds, Some(1800));
}

fn single_step_with_cache(cache_json: &str) -> RawCachePolicy {
    let json = format!(
        r#"{{"steps": [{{"cmd": "x", "parent_idx": null, "cache": {cache_json}}}], "leaf_indices": [0]}}"#
    );
    let chain: RawStepChain = serde_json::from_str(&json).unwrap();
    chain.steps.into_iter().next().unwrap().cache.unwrap()
}
