#![allow(
    clippy::unwrap_used,      // test code: panicking asserts are intentional
    clippy::default_trait_access, // `Default::default()` is clear enough in tests
)]

use futures::StreamExt;
use hm_exec::*;
use hm_plugin_protocol::events::{BuildEvent, BuildRef};
use tokio_util::sync::CancellationToken;

#[derive(Debug)]
struct FakeBackend;

#[async_trait::async_trait]
impl ExecutionBackend for FakeBackend {
    fn name(&self) -> &'static str {
        "fake"
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities::local()
    }
    async fn start(&self, _req: RunRequest) -> Result<BackendHandle> {
        let (tx, rx) = tokio::sync::mpsc::channel(8);
        let cancel = CancellationToken::new();
        let build = BuildRef {
            run_id: uuid::Uuid::nil(),
            number: None,
            org: None,
            pipeline: "p".into(),
        };
        let join = tokio::spawn(async move {
            let _ = tx
                .send(BuildEvent::BuildEnd {
                    exit_code: 0,
                    duration_ms: 5,
                })
                .await;
            Ok(BuildOutcome {
                build,
                status: BuildStatus::Passed,
                steps: vec![],
                started_at: chrono::Utc::now(),
                finished_at: chrono::Utc::now(),
                watch_url: None,
            })
        });
        Ok(BackendHandle::spawn(rx, cancel, join))
    }
}

#[tokio::test]
async fn handle_yields_events_then_outcome() {
    let backend: Box<dyn ExecutionBackend> = Box::new(FakeBackend);
    let req = fake_request();
    let handle = backend.start(req).await.unwrap();
    let (mut events, control) = handle.into_parts();
    let mut count = 0;
    while let Some(_ev) = events.next().await {
        count += 1;
    }
    let outcome = control.wait().await.unwrap();
    assert_eq!(count, 1);
    assert_eq!(outcome.status, BuildStatus::Passed);
}

/// Minimal no-op [`hm_vm::VmBackend`] so the local backend can be constructed
/// without a real Docker daemon. `name()`/`capabilities()` never touch it.
#[derive(Debug)]
struct NoopVmBackend;

#[async_trait::async_trait]
impl hm_vm::VmBackend for NoopVmBackend {
    async fn create(
        &self,
        _image: &str,
        _config: &hm_vm::VmConfig,
    ) -> anyhow::Result<Box<dyn hm_vm::backend::Vm>> {
        anyhow::bail!("noop backend")
    }
    async fn restore(
        &self,
        _snapshot: &hm_vm::SnapshotId,
        _config: &hm_vm::VmConfig,
    ) -> anyhow::Result<Box<dyn hm_vm::backend::Vm>> {
        anyhow::bail!("noop backend")
    }
    async fn snapshot_exists(&self, _snapshot: &hm_vm::SnapshotId) -> anyhow::Result<bool> {
        Ok(false)
    }
    async fn remove_snapshot(&self, _snapshot: &hm_vm::SnapshotId) -> anyhow::Result<()> {
        Ok(())
    }
}

#[tokio::test]
async fn local_backend_reports_capabilities() {
    let b = hm_exec::LocalBackend::new(
        std::num::NonZeroUsize::new(4).unwrap(),
        std::sync::Arc::new(NoopVmBackend),
    );
    assert_eq!(b.name(), "local");
    assert!(b.capabilities().honors_parallelism);
    assert!(b.capabilities().honors_keep_going);
    assert!(!b.capabilities().is_observer);
}

#[test]
fn cloud_backend_capabilities() {
    // `with_base_url` does no network IO, so this is safe with a dummy token.
    let c = hm_exec::CloudBackend::new(
        harmont_cloud::HarmontClient::with_base_url("t", "http://localhost"),
        "http://localhost".into(),
        "http://localhost".into(),
        "acme".into(),
    );
    assert_eq!(c.name(), "cloud");
    assert!(c.capabilities().is_observer);
    assert!(c.capabilities().provides_watch_url);
    assert!(!c.capabilities().honors_parallelism);
    assert!(!c.capabilities().honors_keep_going);
}

fn fake_request() -> RunRequest {
    RunRequest {
        plan: Plan::parse(r#"{"version":"0","graph":{"nodes":[],"node_holes":[],"edge_property":"directed","edges":[]}}"#.into()).unwrap(),
        repo_root: std::path::PathBuf::from("/tmp"),
        pipeline_slug: "p".into(),
        env: Default::default(),
        source: SourceMeta {
            branch: "main".into(),
            commit: "0".repeat(40),
            message: None,
            repo_name: None,
        },
        options: RunOptions {
            watch: true,
            ..Default::default()
        },
    }
}
