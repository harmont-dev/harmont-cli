//! Docker-based step runner.
//!
//! Each step runs inside a Docker container with the workspace
//! bind-mounted from the host via COW clones. System-level state
//! (installed packages) propagates via Docker image commits; workspace
//! files propagate via host-side COW directory clones.

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
// Step execution
// ---------------------------------------------------------------------------

fn resolve_image(step: &CommandStep, input: &ExecutorInput) -> String {
    if let Some(snap) = &input.parent_snapshot {
        return snap.to_string();
    }
    step.image
        .clone()
        .unwrap_or_else(|| "alpine:latest".to_string())
}

async fn run_step(ctx: &RunContext, input: ExecutorInput) -> Result<StepResult> {
    let plan = decision_plan(&input.cache_lookup);

    if !plan.run_command {
        return Ok(StepResult {
            exit_code: 0,
            committed_snapshot: plan.hit_tag.clone(),
            artifacts: vec![],
        });
    }

    let workspace_mgr = &ctx.workspace;

    let workspace_path = {
        let mgr = workspace_mgr
            .lock()
            .map_err(|_| anyhow::anyhow!("workspace manager mutex poisoned"))?;
        mgr.workspace_path(&input.step.key)
            .map(std::path::Path::to_path_buf)
            .ok_or_else(|| anyhow::anyhow!("workspace for step '{}' not created", input.step.key))?
    };

    let image = resolve_image(&input.step, &input);
    let container_name = sanitize_container_name(&input.run_id.to_string(), &input.step.key);
    let env_vec: Vec<String> = input.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

    // Pull image if needed.
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

    // Start container with workspace bind mount.
    let binds = vec![format!("{}:{}", workspace_path.display(), input.workdir)];
    let cid = ctx
        .docker
        .start_long_lived_with_mounts(&image, &env_vec, &input.workdir, &container_name, &binds)
        .await
        .context("docker start with mounts failed")?;

    let result = run_in_container(ctx, &cid, &input, &env_vec, &plan).await;
    ctx.docker.stop_remove(&cid).await;
    result
}

async fn run_in_container(
    ctx: &RunContext,
    cid: &str,
    input: &ExecutorInput,
    env_vec: &[String],
    plan: &DecisionPlan,
) -> Result<StepResult> {
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

    // Commit container so child steps inherit system-level changes
    // (installed packages, etc.). Workspace files propagate via COW
    // bind mounts, but the container image captures everything else.
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
        match ctx
            .docker
            .commit_container(cid, &target_tag.to_string())
            .await
        {
            Ok(_) => Some(target_tag),
            Err(e) => {
                tracing::warn!(step_key = %input.step.key, "docker commit failed, step still succeeded: {e:#}");
                None
            }
        }
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

    fn make_input(step: CommandStep, parent_snapshot: Option<SnapshotRef>) -> ExecutorInput {
        ExecutorInput {
            step,
            workspace_archive_id: hm_plugin_protocol::ArchiveId::from(uuid::Uuid::nil()),
            env: std::collections::BTreeMap::new(),
            workdir: "/workspace".into(),
            run_id: uuid::Uuid::nil(),
            step_id: uuid::Uuid::nil(),
            cache_lookup: CacheDecision::MissNoCommit,
            parent_snapshot,
        }
    }

    // -- resolve_image ----------------------------------------------------

    #[test]
    fn resolve_image_uses_step_image() {
        let s = step_with_image(Some("rust:1.82"));
        let input = make_input(s.clone(), None);
        assert_eq!(resolve_image(&s, &input), "rust:1.82");
    }

    #[test]
    fn resolve_image_fallback_alpine() {
        let s = step_with_image(None);
        let input = make_input(s.clone(), None);
        assert_eq!(resolve_image(&s, &input), "alpine:latest");
    }

    #[test]
    fn resolve_image_prefers_parent_snapshot() {
        let s = step_with_image(Some("rust:1.82"));
        let snap = SnapshotRef::from("harmont-local-ephemeral/base:abc123".to_string());
        let input = make_input(s.clone(), Some(snap));
        assert_eq!(
            resolve_image(&s, &input),
            "harmont-local-ephemeral/base:abc123"
        );
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
