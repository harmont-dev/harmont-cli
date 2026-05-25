//! Docker-based step runner.
//!
//! Replaces the old `hm-plugin-docker` WASM plugin with direct Bollard
//! calls. All Docker orchestration (pull, start, extract, exec, commit,
//! stop+remove) runs through [`RunContext::docker`] with cancellation
//! support via [`RunContext::cancel`].

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, Result};
use hm_plugin_protocol::{
    BuildEvent, CacheDecision, CommandStep, ExecutorInput, SnapshotRef, StdStream, StepResult,
};
use uuid::Uuid;

use super::{RunContext, StepRunner};
use crate::orchestrator::events::EventBus;

// ---------------------------------------------------------------------------
// EXTRACT_CMD_SH
// ---------------------------------------------------------------------------

/// Shell script for idempotent workspace extraction. Reads a `.harmont-extracted`
/// manifest to clean up files from a previous extract, then unpacks the new
/// archive and writes a fresh manifest. Files created by the step command
/// (e.g. `node_modules`, build artifacts) are not tracked and survive untouched.
const EXTRACT_CMD_SH: &str = r#"set -e
mkdir -p "$WORKDIR"
cd "$WORKDIR"
manifest="$WORKDIR/.harmont-extracted"
if [ -f "$manifest" ]; then
  # Longest paths first: removes nested entries before their parents.
  sort -r "$manifest" | while IFS= read -r p; do
    [ -n "$p" ] || continue
    if [ -d "$p" ] && [ ! -L "$p" ]; then
      rmdir "$p" 2>/dev/null || true
    else
      rm -f "$p" 2>/dev/null || true
    fi
  done
  rm -f "$manifest"
fi
# Stream the archive into a temp file so we can both list and extract.
tmp=$(mktemp)
trap 'rm -f "$tmp"' EXIT
cat > "$tmp"
tar -tzf "$tmp" > "$manifest"
tar -xzf "$tmp"
"#;

// ---------------------------------------------------------------------------
// DockerRunner
// ---------------------------------------------------------------------------

/// Step runner that executes pipeline steps inside Docker containers
/// via the local daemon (Bollard).
#[derive(Debug)]
pub struct DockerRunner;

impl StepRunner for DockerRunner {
    fn name(&self) -> &'static str {
        "docker"
    }

    fn execute(
        &self,
        ctx: &RunContext,
        input: ExecutorInput,
    ) -> Pin<Box<dyn Future<Output = Result<StepResult>> + Send + '_>> {
        let ctx = ctx.clone();
        Box::pin(async move { run_step(&ctx, input).await })
    }
}

// ---------------------------------------------------------------------------
// Core orchestration
// ---------------------------------------------------------------------------

async fn run_step(ctx: &RunContext, input: ExecutorInput) -> Result<StepResult> {
    let plan = decision_plan(&input.cache_lookup);

    // Cache hit shortcut: no container, no exec; hand back the hit
    // tag so downstream steps can boot from it.
    if !plan.run_command {
        return Ok(StepResult {
            exit_code: 0,
            committed_snapshot: plan.hit_tag.clone(),
            artifacts: vec![],
        });
    }

    let image = resolve_image(
        &input.step,
        plan.hit_tag.as_ref(),
        input.parent_snapshot.as_ref(),
    );
    let container_name = sanitize_container_name(&input.run_id.to_string(), &input.step.key);
    let env_vec: Vec<String> = input.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

    // Ensure the image is locally available.
    if !ctx.docker.image_exists(&image).await.unwrap_or(false) {
        let docker = ctx.docker.clone();
        let cancel = ctx.cancel.clone();
        let img = image.clone();
        let pull_fut = async move { docker.pull_image(&img).await };
        tokio::select! {
            result = pull_fut => result.with_context(|| format!("pull '{image}'"))?,
            () = cancel.cancelled() => anyhow::bail!("cancelled during image pull"),
        }
    }

    let cid = ctx
        .docker
        .start_long_lived(&image, &env_vec, &input.workdir, &container_name)
        .await
        .context("docker start failed")?;

    // Always stop+remove the container, even on error.
    let result = run_in_container(ctx, &cid, &input, &env_vec, &plan).await;
    ctx.docker.stop_remove(&cid).await;
    result
}

/// Inner body executed with a running container. Separated so the
/// caller can unconditionally clean up the container in all paths.
async fn run_in_container(
    ctx: &RunContext,
    cid: &str,
    input: &ExecutorInput,
    env_vec: &[String],
    plan: &DecisionPlan,
) -> Result<StepResult> {
    // --- Extract workspace archive ---
    let archive = ctx.archives.read(input.workspace_archive_id, 0, u64::MAX);
    if archive.is_empty() {
        anyhow::bail!("archive {} is empty or unknown", input.workspace_archive_id);
    }

    let docker = ctx.docker.clone();
    let cancel = ctx.cancel.clone();
    let cid_owned = cid.to_owned();
    let workdir = input.workdir.clone();
    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        EXTRACT_CMD_SH.replace("$WORKDIR", &workdir),
    ];
    let extract_fut = async move {
        let mut sink = tokio::io::sink();
        let rc = docker
            .exec_streaming_stdin(&cid_owned, &cmd, &[], "/", &archive, &mut sink)
            .await?;
        if rc != 0 {
            anyhow::bail!("tar extract exited {rc}");
        }
        Ok::<(), anyhow::Error>(())
    };
    tokio::select! {
        result = extract_fut => result.context("workspace extract failed")?,
        () = cancel.cancelled() => anyhow::bail!("cancelled during workspace extract"),
    }

    // --- Exec step command ---
    let mut writer = StepLogWriter::new(input.step_id, Arc::clone(&ctx.event_bus));
    let docker = ctx.docker.clone();
    let cancel = ctx.cancel.clone();
    let cid_owned = cid.to_owned();
    let cmd = vec!["sh".into(), "-c".into(), input.step.cmd.clone()];
    let workdir = input.workdir.clone();
    let env_owned = env_vec.to_vec();
    let exec_fut = async move {
        let rc = docker
            .exec_streaming(&cid_owned, &cmd, &env_owned, &workdir, &mut writer)
            .await?;
        writer.flush_remaining();
        Ok::<i64, anyhow::Error>(rc)
    };

    let rc = tokio::select! {
        result = exec_fut => result.context("docker exec failed")?,
        () = cancel.cancelled() => {
            return Ok(StepResult {
                exit_code: 130,
                committed_snapshot: None,
                artifacts: vec![],
            });
        }
    };

    #[allow(
        clippy::cast_possible_truncation,
        reason = "docker exit codes fit in i32"
    )]
    let exit_code = rc as i32;

    // --- Commit snapshot on success ---
    let committed = if exit_code == 0 {
        let target_tag = plan.commit_to.clone().unwrap_or_else(|| {
            let safe: String = input
                .step
                .key
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                        c
                    } else {
                        '-'
                    }
                })
                .collect();
            SnapshotRef::from(format!(
                "harmont-local-ephemeral/{safe}:run-{}",
                input.step_id.simple()
            ))
        });
        ctx.docker
            .commit_container(cid, &target_tag.to_string())
            .await
            .context("docker commit failed")?;
        Some(target_tag)
    } else {
        None
    };

    Ok(StepResult {
        exit_code,
        committed_snapshot: committed,
        artifacts: vec![],
    })
}

// ---------------------------------------------------------------------------
// DecisionPlan
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
struct DecisionPlan {
    run_command: bool,
    commit_to: Option<SnapshotRef>,
    hit_tag: Option<SnapshotRef>,
}

fn decision_plan(decision: &CacheDecision) -> DecisionPlan {
    match decision {
        CacheDecision::Hit { tag } => DecisionPlan {
            run_command: false,
            commit_to: None,
            hit_tag: Some(tag.clone()),
        },
        CacheDecision::MissBuildAs { tag } => DecisionPlan {
            run_command: true,
            commit_to: Some(tag.clone()),
            hit_tag: None,
        },
        CacheDecision::MissNoCommit => DecisionPlan {
            run_command: true,
            commit_to: None,
            hit_tag: None,
        },
    }
}

// ---------------------------------------------------------------------------
// resolve_image
// ---------------------------------------------------------------------------

/// Pick the base image for a step at boot time.
///
/// Priority (high to low):
/// 1. Cache `hit_tag` — the host already located a satisfying snapshot.
/// 2. `parent_snapshot` — the previous step in this chain committed a
///    snapshot; chain-lineage requires we boot from it.
/// 3. The step's `image` field.
/// 4. Fall back to `"alpine:latest"`.
fn resolve_image(
    step: &CommandStep,
    hit_tag: Option<&SnapshotRef>,
    parent_snapshot: Option<&SnapshotRef>,
) -> String {
    if let Some(tag) = hit_tag {
        return tag.to_string();
    }
    if let Some(snap) = parent_snapshot {
        return snap.to_string();
    }
    if let Some(image) = &step.image {
        return image.clone();
    }
    "alpine:latest".to_string()
}

// ---------------------------------------------------------------------------
// sanitize_container_name
// ---------------------------------------------------------------------------

fn sanitize_container_name(run_id: &str, step_key: &str) -> String {
    let run_short: String = run_id.chars().take(8).collect();
    let key: String = step_key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    format!("harmont-{run_short}-{key}")
}

// ---------------------------------------------------------------------------
// StepLogWriter
// ---------------------------------------------------------------------------

/// Streams bytes from a Docker exec into per-line [`BuildEvent::StepLog`]
/// events on the [`EventBus`]. Buffers partial lines until a `\n` arrives.
struct StepLogWriter {
    step_id: Uuid,
    bus: Arc<EventBus>,
    buf: Vec<u8>,
}

impl StepLogWriter {
    fn new(step_id: Uuid, bus: Arc<EventBus>) -> Self {
        Self {
            step_id,
            bus,
            buf: Vec::with_capacity(8192),
        }
    }

    fn flush_line(&self, line: &[u8]) {
        self.bus.emit(BuildEvent::StepLog {
            step_id: self.step_id,
            stream: StdStream::Stdout,
            line: String::from_utf8_lossy(line).into_owned(),
            ts: chrono::Utc::now(),
        });
    }

    fn flush_remaining(&mut self) {
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            self.flush_line(&line);
        }
    }
}

impl tokio::io::AsyncWrite for StepLogWriter {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let len = buf.len();
        for b in buf {
            if *b == b'\n' {
                let line = std::mem::take(&mut self.buf);
                self.flush_line(&line);
            } else {
                self.buf.push(*b);
            }
        }
        std::task::Poll::Ready(Ok(len))
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.flush_remaining();
        std::task::Poll::Ready(Ok(()))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    use hm_plugin_protocol::CacheDecision;

    fn step_with_image(image: Option<&str>) -> CommandStep {
        CommandStep {
            key: "k".into(),
            label: None,
            cmd: "true".into(),
            image: image.map(String::from),
            env: None,
            timeout_seconds: None,
            cache: None,
            runner: None,
            runner_args: None,
        }
    }

    // -- resolve_image -------------------------------------------------------

    #[test]
    fn resolve_image_hit_tag_wins() {
        let s = step_with_image(Some("rust:1.82"));
        let hit = SnapshotRef("cache:tag".into());
        let parent = SnapshotRef("parent:tag".into());
        assert_eq!(resolve_image(&s, Some(&hit), Some(&parent)), "cache:tag");
    }

    #[test]
    fn resolve_image_parent_snapshot_beats_step_image() {
        let s = step_with_image(Some("rust:1.82"));
        let parent = SnapshotRef("parent:tag".into());
        assert_eq!(resolve_image(&s, None, Some(&parent)), "parent:tag");
    }

    #[test]
    fn resolve_image_step_image_used() {
        let s = step_with_image(Some("rust:1.82"));
        assert_eq!(resolve_image(&s, None, None), "rust:1.82");
    }

    #[test]
    fn resolve_image_fallback_alpine() {
        let s = step_with_image(None);
        assert_eq!(resolve_image(&s, None, None), "alpine:latest");
    }

    // -- decision_plan -------------------------------------------------------

    #[test]
    fn decision_hit_skips_command() {
        let plan = decision_plan(&CacheDecision::Hit {
            tag: SnapshotRef("cached:v1".into()),
        });
        assert!(!plan.run_command);
        assert!(plan.commit_to.is_none());
        assert_eq!(plan.hit_tag.as_ref().unwrap().0, "cached:v1");
    }

    #[test]
    fn decision_miss_build_as_runs_and_commits() {
        let plan = decision_plan(&CacheDecision::MissBuildAs {
            tag: SnapshotRef("build:v2".into()),
        });
        assert!(plan.run_command);
        assert_eq!(plan.commit_to.as_ref().unwrap().0, "build:v2");
        assert!(plan.hit_tag.is_none());
    }

    #[test]
    fn decision_miss_no_commit() {
        let plan = decision_plan(&CacheDecision::MissNoCommit);
        assert!(plan.run_command);
        assert!(plan.commit_to.is_none());
        assert!(plan.hit_tag.is_none());
    }

    // -- sanitize_container_name ---------------------------------------------

    #[test]
    fn sanitize_container_name_replaces_special_chars() {
        let name = sanitize_container_name("abcdef12-3456-7890", "my/step.key:v1");
        assert_eq!(name, "harmont-abcdef12-my-step-key-v1");
    }

    #[test]
    fn sanitize_container_name_preserves_valid_chars() {
        let name = sanitize_container_name("run-id-1234", "normal_step-key");
        assert_eq!(name, "harmont-run-id-1-normal_step-key");
    }

    // -- StepLogWriter -------------------------------------------------------

    #[tokio::test]
    async fn step_log_writer_emits_on_newline() {
        use tokio::io::AsyncWriteExt;

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let step_id = Uuid::new_v4();

        let mut writer = StepLogWriter::new(step_id, bus);
        writer.write_all(b"hello\nworld\n").await.unwrap();

        let ev1 = rx.recv().await.unwrap();
        let ev2 = rx.recv().await.unwrap();

        match ev1 {
            BuildEvent::StepLog {
                step_id: sid, line, ..
            } => {
                assert_eq!(sid, step_id);
                assert_eq!(line, "hello");
            }
            other => panic!("expected StepLog, got {other:?}"),
        }
        match ev2 {
            BuildEvent::StepLog { line, .. } => assert_eq!(line, "world"),
            other => panic!("expected StepLog, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn step_log_writer_flushes_remaining_on_shutdown() {
        use tokio::io::AsyncWriteExt;

        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        let step_id = Uuid::new_v4();

        let mut writer = StepLogWriter::new(step_id, bus);
        // Write partial line without trailing newline.
        writer.write_all(b"partial").await.unwrap();
        writer.shutdown().await.unwrap();

        let ev = rx.recv().await.unwrap();
        match ev {
            BuildEvent::StepLog { line, .. } => assert_eq!(line, "partial"),
            other => panic!("expected StepLog, got {other:?}"),
        }
    }
}
