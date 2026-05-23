#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use hm_pipeline_ir::{Pipeline, Step};

#[test]
fn parses_step_with_runner() {
    let json = br#"{
        "version": "0",
        "steps": [
            {"type": "command", "key": "a", "cmd": "echo a"},
            {"type": "command", "key": "b", "cmd": "freestyle run",
             "runner": "freestyle", "runner_args": {"region": "us"}}
        ]
    }"#;
    let p: Pipeline = serde_json::from_slice(json).unwrap();
    let Step::Command(b) = &p.steps[1] else {
        panic!("expected command")
    };
    assert_eq!(b.runner.as_deref(), Some("freestyle"));
    assert_eq!(b.runner_args.as_ref().unwrap()["region"], "us");
}

#[test]
fn parses_legacy_step_without_runner() {
    let json = br#"{
        "version": "0",
        "steps": [{"type": "command", "key": "a", "cmd": "echo a"}]
    }"#;
    let p: Pipeline = serde_json::from_slice(json).unwrap();
    let Step::Command(a) = &p.steps[0] else {
        panic!("expected command")
    };
    assert!(a.runner.is_none());
    assert!(a.runner_args.is_none());
}
