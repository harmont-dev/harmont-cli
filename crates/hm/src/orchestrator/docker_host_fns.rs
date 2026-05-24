//! Bollard-backed implementations of the `hm_docker_*` host fns.
//!
//! These wrap [`crate::orchestrator::docker_client::DockerClient`]. The
//! docker step-executor plugin calls these via Extism host-fn imports.

use anyhow::{Context, Result};
use hm_plugin_protocol::{DockerCommitArgs, DockerExecArgs, DockerExtractArgs, DockerStartArgs};

use super::state::current;

// Workspace extract must be idempotent across snapshot reuse: when a
// parent snapshot is shared across different repos (e.g. an apt-base
// step's image cached on apt-package set only, then reused by two
// example projects), the previous repo's files in $WORKDIR would
// otherwise leak into the new run because `tar -xzf` overlays rather
// than mirrors. To keep this surgical, every extract writes a manifest
// of the paths it laid down to `$WORKDIR/.harmont-extracted`. The next
// extract reads that manifest, deletes only the paths the previous
// extract added (longest first so files go before their parent dirs),
// then unpacks the new archive (writing a fresh manifest). Files
// created inside the container by a step's command (e.g. `node_modules`
// after `npm ci`, build artifacts under `build/`) are not in any
// manifest, so they survive untouched — preserving the intra-chain
// artifact-passing semantics that toolchains rely on.
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

pub(crate) async fn ping_impl() -> bool {
    let Some(s) = current() else {
        return false;
    };
    s.docker.ping().await.is_ok()
}

pub(crate) async fn image_exists_impl(tag: String) -> bool {
    let Some(s) = current() else { return false };
    s.docker.image_exists(&tag).await.unwrap_or(false)
}

pub(crate) async fn pull_impl(tag: String) -> Result<()> {
    let s = current().context("no orchestrator state")?;
    let cancel = s.cancel.clone();
    let docker = s.docker.clone();
    let pull_fut = async move { docker.pull_image(&tag).await };
    tokio::select! {
        result = pull_fut => result,
        () = wait_cancel(&cancel) => Err(anyhow::anyhow!("cancelled during image pull")),
    }
}

pub(crate) async fn start_container_impl(args: DockerStartArgs) -> Result<String> {
    let s = current().context("no orchestrator state")?;
    let env_vec: Vec<String> = args
        .env
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    s.docker
        .start_long_lived(&args.image, &env_vec, &args.workdir, &args.name_hint)
        .await
}

pub(crate) async fn extract_workspace_impl(args: DockerExtractArgs) -> Result<()> {
    let s = current().context("no orchestrator state")?;
    let archive = s.archives.read(args.archive_id, 0, u64::MAX);
    if archive.is_empty() {
        anyhow::bail!("archive {} is empty or unknown", args.archive_id);
    }
    let cancel = s.cancel.clone();
    let docker = s.docker.clone();
    let cid = args.container_id;
    let workdir = args.workdir;
    let cmd = vec![
        "sh".to_string(),
        "-c".to_string(),
        EXTRACT_CMD_SH.replace("$WORKDIR", &workdir),
    ];
    let extract_fut = async move {
        let mut sink = tokio::io::sink();
        let rc = docker
            .exec_streaming_stdin(&cid, &cmd, &[], "/", &archive, &mut sink)
            .await?;
        if rc != 0 {
            anyhow::bail!("tar extract exited {rc}");
        }
        Ok::<(), anyhow::Error>(())
    };
    tokio::select! {
        result = extract_fut => result,
        () = wait_cancel(&cancel) => Err(anyhow::anyhow!("cancelled during workspace extract")),
    }
}

pub(crate) async fn exec_impl(args: DockerExecArgs) -> Result<i32> {
    let s = current().context("no orchestrator state")?;
    let env_vec: Vec<String> = args
        .env
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    // Emit StepLog events for each line written; the writer below
    // forwards bytes into the event bus tagged with the current
    // thread-local step_id set by the scheduler.
    let mut writer = StepLogWriter::default();

    // Future doing the exec; we race it against cancellation.
    let cancel = s.cancel.clone();
    let docker = s.docker.clone();
    let cid = args.container_id.clone();
    let cmd = args.cmd.clone();
    let workdir = args.workdir.clone();
    let archive_opt = args.stdin_archive_id;
    let archive_bytes = archive_opt.map(|id| s.archives.read(id, 0, u64::MAX));

    let exec_fut = async move {
        let rc = match archive_bytes {
            Some(bytes) => {
                docker
                    .exec_streaming_stdin(&cid, &cmd, &env_vec, &workdir, &bytes, &mut writer)
                    .await?
            }
            None => {
                docker
                    .exec_streaming(&cid, &cmd, &env_vec, &workdir, &mut writer)
                    .await?
            }
        };
        writer.flush_remaining();
        Ok::<i64, anyhow::Error>(rc)
    };

    let rc = tokio::select! {
        result = exec_fut => result?,
        () = wait_cancel(&cancel) => {
            // Cancelled. Try to bail with the conventional sigint code.
            return Ok(130);
        }
    };
    i32::try_from(rc).context("docker exit code out of i32 range")
}

async fn wait_cancel(cancel: &tokio_util::sync::CancellationToken) {
    cancel.cancelled().await;
}

pub(crate) async fn commit_impl(args: DockerCommitArgs) -> Result<String> {
    let s = current().context("no orchestrator state")?;
    s.docker
        .commit_container(&args.container_id, &args.tag)
        .await
}

pub(crate) async fn remove_image_impl(tag: String) -> Result<()> {
    let s = current().context("no orchestrator state")?;
    s.docker.remove_image(&tag).await
}

pub(crate) async fn stop_remove_impl(container_id: String) {
    if let Some(s) = current() {
        s.docker.stop_remove(&container_id).await;
    }
}

/// Streams bytes from a Docker exec into per-line `StepLog` events on
/// the event bus. Buffers partial lines until a `\n` arrives.
#[derive(smart_default::SmartDefault)]
struct StepLogWriter {
    #[default(Vec::with_capacity(8192))]
    buf: Vec<u8>,
}

impl StepLogWriter {
    fn flush_line(line: &[u8]) {
        let Some(state) = current() else { return };
        let Some(step_id) = crate::plugin::host_fns::current_step_id() else {
            return;
        };
        state
            .event_bus
            .emit(hm_plugin_protocol::BuildEvent::StepLog {
                step_id,
                stream: hm_plugin_protocol::StdStream::Stdout,
                line: String::from_utf8_lossy(line).into_owned(),
                ts: chrono::Utc::now(),
            });
    }

    fn flush_remaining(&mut self) {
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            Self::flush_line(&line);
        }
    }
}

impl tokio::io::AsyncWrite for StepLogWriter {
    fn poll_write(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        let len = buf.len();
        for b in buf {
            if *b == b'\n' {
                let line = std::mem::take(&mut self.buf);
                Self::flush_line(&line);
            } else {
                self.buf.push(*b);
            }
        }
        std::task::Poll::Ready(Ok(len))
    }

    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(
        mut self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        self.flush_remaining();
        std::task::Poll::Ready(Ok(()))
    }
}
