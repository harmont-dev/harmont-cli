//! VM-based step runner.
//!
//! Each step runs inside a lightweight VM managed by [`HmVm`]. The
//! source archive is extracted to a host-side temp directory and
//! injected into the VM before the step command runs. System-level
//! state propagates via VM snapshots.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result};
use hm_plugin_protocol::{
    BuildEvent, CacheDecision, ExecutorInput, SnapshotRef, StdStream, StepResult,
};
use hm_vm::types::OutputSink;
use hm_vm::{Action, CachingPolicy, HmVm, ImageSource, SnapshotId};
use uuid::Uuid;

use super::{StepContext, StepRunner};
use crate::local::events::EventBus;

/// Step runner that executes pipeline steps inside lightweight VMs
/// via the [`HmVm`] orchestrator.
#[derive(Debug)]
pub struct VmRunner {
    vm: Arc<HmVm>,
}

impl VmRunner {
    /// Create a new `VmRunner` backed by the given VM orchestrator.
    #[must_use]
    pub const fn new(vm: Arc<HmVm>) -> Self {
        Self { vm }
    }
}

impl StepRunner for VmRunner {
    fn name(&self) -> &'static str {
        "vm"
    }

    fn execute(
        &self,
        ctx: &StepContext,
        input: ExecutorInput,
    ) -> Pin<Box<dyn Future<Output = Result<StepResult>> + Send + '_>> {
        let ctx = ctx.clone();
        let vm = Arc::clone(&self.vm);
        Box::pin(async move { run_step_vm(&vm, &ctx, input).await })
    }

    fn reap_snapshots<'a>(
        &'a self,
        snapshots: Vec<SnapshotRef>,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        let vm = Arc::clone(&self.vm);
        Box::pin(async move {
            for snap in snapshots {
                let id = SnapshotId::new(snap.0);
                if let Err(e) = vm.remove_snapshot(&id).await {
                    tracing::warn!(snapshot = %id, error = %e, "failed to reap ephemeral snapshot");
                }
            }
        })
    }
}

#[tracing::instrument(skip(vm, ctx), fields(step_key = %input.step.key))]
async fn run_step_vm(vm: &HmVm, ctx: &StepContext, input: ExecutorInput) -> Result<StepResult> {
    let policy = match &input.cache_lookup {
        CacheDecision::Hit { tag } | CacheDecision::MissBuildAs { tag } => {
            CachingPolicy::Cache { key: tag.0.clone() }
        }
        CacheDecision::MissNoCommit => CachingPolicy::None,
    };

    let source = if let Some(ref snap) = input.parent_snapshot {
        ImageSource::Snapshot(SnapshotId::new(snap.0.clone()))
    } else {
        // Imageless root step: default to an apt-capable base. The SDK's
        // toolchains all assume `apt-get`, so `ubuntu:24.04` is the
        // across-the-board default (alpine has no apt-get).
        ImageSource::Image(
            input
                .step
                .image
                .clone()
                .unwrap_or_else(|| "ubuntu:24.04".to_string()),
        )
    };

    // Inject the current workspace on every executing step, overlaying it
    // onto the system state inherited from the parent snapshot (apt packages,
    // installed runtimes, `node_modules`, …). Injecting only at the chain root
    // is wrong: root steps such as `apt_base` are `CacheForever`, so their
    // snapshots freeze the source tree captured at first build and every COW
    // descendant inherits that stale tree — source edits never reach leaf
    // steps. A true cache hit short-circuits inside `HmVm::execute` before
    // inject runs, so this overlay only happens when a step actually executes;
    // the overlay (Docker PUT-archive) adds/overwrites files without deleting
    // the inherited system state.
    let (inject, _temp_guard) = {
        let archive_bytes = ctx
            .archives
            .get_bytes(input.workspace_archive_id)
            .ok_or_else(|| anyhow::anyhow!("source archive not found"))?;
        let dir =
            extract_archive_to_tempdir(&archive_bytes).context("extracting workspace archive")?;
        let path = dir.path().to_path_buf();
        (Some(path), Some(dir))
    };

    // Baseline env for shell operation inside VMs.
    let mut env: Vec<(String, String)> = vec![
        ("HOME".into(), "/root".into()),
        (
            "PATH".into(),
            "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin".into(),
        ),
    ];
    env.extend(input.env);

    let action = Action {
        source,
        cmd: input.step.cmd.clone(),
        env,
        working_dir: input.workdir.clone(),
        timeout: None,
        inject,
    };

    let sink = EventBusSink {
        step_id: input.step_id,
        bus: Arc::clone(&ctx.event_bus),
    };

    let result = tokio::select! {
        r = vm.execute(action, policy, &sink) => r,
        () = ctx.cancel.cancelled() => {
            anyhow::bail!("step cancelled (build timeout or sibling failure)")
        }
    }
    .context("vm execute failed")?;

    if result.cached {
        ctx.event_bus.emit(BuildEvent::StepCacheHit {
            step_id: input.step_id,
            key: input
                .step
                .cache
                .as_ref()
                .and_then(|c| c.key.clone())
                .unwrap_or_default(),
            tag: result
                .snapshot
                .as_ref()
                .map_or_else(String::new, ToString::to_string),
        });
    }

    Ok(StepResult {
        exit_code: result.exit_code,
        committed_snapshot: result.snapshot.map(|s| SnapshotRef(s.to_string())),
        artifacts: vec![],
    })
}

/// Extracts a gzipped tar archive into a temporary directory.
fn extract_archive_to_tempdir(archive_bytes: &[u8]) -> Result<tempfile::TempDir> {
    let temp_dir = tempfile::tempdir().context("creating temp directory")?;
    let decoder = flate2::read::GzDecoder::new(archive_bytes);
    let mut archive = tar::Archive::new(decoder);
    archive
        .unpack(temp_dir.path())
        .context("unpacking archive")?;
    Ok(temp_dir)
}

/// [`OutputSink`] implementation that emits [`BuildEvent::StepLog`]
/// events on the [`EventBus`].
struct EventBusSink {
    step_id: Uuid,
    bus: Arc<EventBus>,
}

impl std::fmt::Debug for EventBusSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventBusSink")
            .field("step_id", &self.step_id)
            .finish_non_exhaustive()
    }
}

impl OutputSink for EventBusSink {
    fn on_stdout(&self, line: &str) {
        self.bus.emit(BuildEvent::StepLog {
            step_id: self.step_id,
            stream: StdStream::Stdout,
            line: line.to_owned(),
            ts: chrono::Utc::now(),
        });
    }

    fn on_stderr(&self, line: &str) {
        self.bus.emit(BuildEvent::StepLog {
            step_id: self.step_id,
            stream: StdStream::Stderr,
            line: line.to_owned(),
            ts: chrono::Utc::now(),
        });
    }
}
