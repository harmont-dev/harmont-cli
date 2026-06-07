//! Integration tests for the boxlite backend.
//! Gate: set HM_VM_INTEGRATION=1 to run.

use std::sync::Arc;

use hm_vm::boxlite::BoxliteBackend;
use hm_vm::{
    Action, CachingPolicy, HmVm, ImageRegistry, ImageSource, NullSink, VmConfig,
};

fn skip_unless_enabled() -> bool {
    std::env::var("HM_VM_INTEGRATION").is_err()
}

#[tokio::test]
async fn create_exec_snapshot_restore() {
    if skip_unless_enabled() {
        return;
    }

    let backend = BoxliteBackend::with_defaults().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let registry = ImageRegistry::open(&tmp.path().join("test.db"), 10).unwrap();
    let vm = HmVm::new(Arc::new(backend), registry, VmConfig::default());

    // Step 1: execute a command that writes a file
    let action = Action {
        source: ImageSource::Image("alpine:latest".into()),
        cmd: "mkdir -p /workspace && echo hello > /workspace/out.txt".into(),
        env: vec![],
        working_dir: "/workspace".into(),
        timeout: None,
        inject: None,
    };
    let result = vm
        .execute(action, CachingPolicy::Cache { key: "test-1".into() }, &NullSink)
        .await
        .unwrap();
    assert_eq!(result.exit_code, 0);
    assert!(result.snapshot.is_some());
    assert!(!result.cached);

    // Step 2: restore from snapshot, verify file exists
    let snap = result.snapshot.unwrap();
    let action2 = Action {
        source: ImageSource::Snapshot(snap),
        cmd: "cat /workspace/out.txt".into(),
        env: vec![],
        working_dir: "/workspace".into(),
        timeout: None,
        inject: None,
    };
    let result2 = vm
        .execute(action2, CachingPolicy::None, &NullSink)
        .await
        .unwrap();
    assert_eq!(result2.exit_code, 0);
}

#[tokio::test]
async fn cache_hit_returns_immediately() {
    if skip_unless_enabled() {
        return;
    }

    let backend = BoxliteBackend::with_defaults().unwrap();
    let tmp = tempfile::TempDir::new().unwrap();
    let registry = ImageRegistry::open(&tmp.path().join("test.db"), 10).unwrap();
    let vm = HmVm::new(Arc::new(backend), registry, VmConfig::default());

    // First run: cache miss
    let action = Action {
        source: ImageSource::Image("alpine:latest".into()),
        cmd: "echo cached".into(),
        env: vec![],
        working_dir: "/".into(),
        timeout: None,
        inject: None,
    };
    let r1 = vm
        .execute(
            action.clone(),
            CachingPolicy::Cache { key: "cached-test".into() },
            &NullSink,
        )
        .await
        .unwrap();
    assert!(!r1.cached);

    // Second run: cache hit (same key)
    let r2 = vm
        .execute(
            action,
            CachingPolicy::Cache { key: "cached-test".into() },
            &NullSink,
        )
        .await
        .unwrap();
    assert!(r2.cached);
    assert_eq!(r2.exit_code, 0);
}
