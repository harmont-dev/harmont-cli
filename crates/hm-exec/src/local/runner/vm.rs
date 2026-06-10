//! VM-based step runner.
//!
//! Each step runs inside a lightweight VM managed by [`HmVm`]. The
//! source archive is extracted to a host-side temp directory and
//! bind-mounted into the VM before the step command runs. System-level
//! state propagates via VM snapshots.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result};
use hm_plugin_protocol::{
    BuildEvent, CacheDecision, ExecutorInput, SnapshotRef, StdStream, StepResult,
};
use hm_vm::types::OutputSink;
use hm_vm::{Action, CachingPolicy, HmVm, ImageSource, SnapshotId, WorkspaceMount};
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
}

#[allow(clippy::too_many_lines)]
#[tracing::instrument(skip(vm, ctx), fields(step_key = %input.step.key))]
async fn run_step_vm(vm: &HmVm, ctx: &StepContext, input: ExecutorInput) -> Result<StepResult> {
    let policy = match &input.cache_lookup {
        CacheDecision::Hit { tag } | CacheDecision::MissBuildAs { tag } => {
            CachingPolicy::Cache { key: tag.0.clone() }
        }
        CacheDecision::MissNoCommit => CachingPolicy::None,
    };

    // Fast path: check cache before doing any workspace prep. COW copies
    // are expensive and entirely wasted on cache hits.
    if let CachingPolicy::Cache { ref key } = policy
        && let Some(result) = vm.peek_cache(key).await?
    {
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
        // Cache hits carry no workspace: workspace state is strictly
        // run-scoped, so children rebase onto the current source instead
        // of inheriting a stale tree from the original run.
        return Ok(StepResult {
            exit_code: 0,
            committed_snapshot: result.snapshot.map(|s| SnapshotRef(s.to_string())),
            artifacts: vec![],
            workspace_dir: None,
            ephemeral_snapshot: false,
        });
    }

    let source = if let Some(ref snap) = input.parent_snapshot {
        ImageSource::Snapshot(SnapshotId::new(snap.0.clone()))
    } else {
        ImageSource::Image(
            input
                .step
                .image
                .clone()
                .unwrap_or_else(|| "alpine:latest".to_string()),
        )
    };

    // Prepare the workspace: COW-copy from the parent step's live workspace
    // (child of a step that executed this run) or from the shared
    // once-per-run source base (root step, or child of a cache-hit parent).
    // Either way, bind-mount the result into the VM. This overlays the
    // current source onto the system state inherited from the parent
    // snapshot on every executing step, so source edits always reach leaf
    // steps even when ancestors are `CacheForever` and froze an older tree.
    let step_ws = tempfile::tempdir().context("creating step workspace")?;

    let cow_src: std::path::PathBuf = if let Some(ref parent_ws) = ctx.parent_workspace_dir {
        std::path::PathBuf::from(parent_ws)
    } else {
        let base = ctx
            .source_base
            .get_or_try_init(|| async {
                let archive_bytes = ctx
                    .archives
                    .get_bytes(input.workspace_archive_id)
                    .ok_or_else(|| anyhow::anyhow!("source archive not found"))?;
                tokio::task::spawn_blocking(move || extract_archive_to_tempdir(&archive_bytes))
                    .await
                    .context("archive extraction task panicked")?
                    .context("extracting workspace archive")
            })
            .await?;
        base.path().to_path_buf()
    };

    {
        let dst = step_ws.path().to_path_buf();
        let src = cow_src.clone();
        tokio::task::spawn_blocking(move || hm_vm::workspace::cow_copy(&src, &dst))
            .await
            .context("workspace COW task panicked")?
            .with_context(|| format!("COW copy {} into step workspace", cow_src.display()))?;
    }

    let workspace = Some(WorkspaceMount {
        host_path: step_ws.path().to_path_buf(),
        guest_path: input.workdir.clone(),
    });

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
        workspace,
    };

    let sink = EventBusSink {
        step_id: input.step_id,
        bus: Arc::clone(&ctx.event_bus),
    };

    // Cancellation is cooperative INSIDE `HmVm::execute` (the token is
    // threaded down): on Ctrl-C / sibling failure / step timeout it bails
    // with exit 130 only after destroying the container, which reclaims
    // bind-mount ownership of root-written files. Never `select!`-drop
    // this future: doing so would tear down `step_ws` concurrently with a
    // still-running container and leak a (root-owned, on native Linux)
    // workspace directory.
    let result = vm
        .execute(action, policy, &sink, &ctx.cancel)
        .await
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

    // Steps that executed successfully (cached-miss and uncached alike)
    // keep their live tempdir alive so same-run children can COW-copy
    // from it; the scheduler removes every kept dir after the DAG drains.
    // Cache hits (rare race: a concurrent fill between peek and execute)
    // and failures propagate no workspace -- the TempDir self-cleans.
    let workspace_dir =
        (result.exit_code == 0 && !result.cached).then(|| step_ws.keep().display().to_string());

    Ok(StepResult {
        exit_code: result.exit_code,
        committed_snapshot: result.snapshot.map(|s| SnapshotRef(s.to_string())),
        artifacts: vec![],
        workspace_dir,
        ephemeral_snapshot: result.ephemeral_snapshot,
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
