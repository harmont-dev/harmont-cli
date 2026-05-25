//! Serde round-trip property tests. Any type the wire uses must be
//! losslessly serialisable through `serde_json`.

#![allow(
    clippy::cargo_common_metadata,
    clippy::multiple_crate_versions,
    clippy::default_trait_access,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic
)]

use hm_plugin_protocol::*;
use uuid::Uuid;

fn rt<T>(v: &T) -> T
where
    T: serde::Serialize + serde::de::DeserializeOwned + PartialEq + std::fmt::Debug,
{
    let s = serde_json::to_string(v).expect("serialize");
    let back: T = serde_json::from_str(&s).expect("deserialize");
    assert_eq!(v, &back, "round-trip mismatch via JSON: {s}");
    back
}

#[test]
fn executor_input_round_trip() {
    let inp = ExecutorInput {
        step: CommandStep {
            key: "build".into(),
            label: None,
            cmd: "cargo build".into(),
            image: Some("rust:1.82".into()),
            env: None,
            timeout_seconds: None,
            cache: None,
            runner: Some("docker".into()),
            runner_args: None,
        },
        workspace_archive_id: ArchiveId(Uuid::nil()),
        env: Default::default(),
        workdir: "/workspace".into(),
        run_id: Uuid::nil(),
        step_id: Uuid::nil(),
        cache_lookup: CacheDecision::MissNoCommit,
        parent_snapshot: None,
    };
    rt(&inp);
}

#[test]
fn build_event_round_trip_all_variants() {
    let evs = vec![
        BuildEvent::BuildStart {
            run_id: Uuid::nil(),
            plan: PlanSummary {
                step_count: 3,
                chain_count: 2,
                default_runner: "docker".into(),
            },
            started_at: chrono::Utc::now(),
        },
        BuildEvent::StepQueued {
            step_id: Uuid::nil(),
            key: "a".into(),
            chain_idx: 0,
            parent_key: None,
            display_name: "a".into(),
        },
        BuildEvent::StepStart {
            step_id: Uuid::nil(),
            runner: "docker".into(),
            image: None,
        },
        BuildEvent::StepLog {
            step_id: Uuid::nil(),
            stream: StdStream::Stdout,
            line: "hi".into(),
            ts: chrono::Utc::now(),
        },
        BuildEvent::StepCacheHit {
            step_id: Uuid::nil(),
            key: "k".into(),
            tag: "t".into(),
        },
        BuildEvent::StepEnd {
            step_id: Uuid::nil(),
            exit_code: 0,
            duration_ms: 1,
            snapshot: None,
        },
        BuildEvent::ChainFailed {
            chain_idx: 1,
            failed_step_id: Uuid::nil(),
            failed_step_key: "build".into(),
            exit_code: 2,
            message: "step exited non-zero".into(),
            ts: chrono::Utc::now(),
        },
        BuildEvent::BuildEnd {
            exit_code: 0,
            duration_ms: 2,
        },
    ];
    for e in &evs {
        rt(e);
    }
}

#[test]
fn cache_decision_round_trip_all_variants() {
    rt(&CacheDecision::Hit {
        tag: SnapshotRef("img:tag".into()),
    });
    rt(&CacheDecision::MissBuildAs {
        tag: SnapshotRef("img:tag".into()),
    });
    rt(&CacheDecision::MissNoCommit);
}
