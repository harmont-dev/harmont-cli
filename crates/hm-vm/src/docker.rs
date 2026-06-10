//! Docker backend -- container orchestration via bollard.
//!
//! Each "VM" is a long-lived container running `sleep infinity`,
//! commands are executed via the exec API, and snapshots are Docker
//! image commits.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result};
use async_trait::async_trait;
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::{
    CommitContainerOptions, CreateImageOptions, ListImagesOptions, RemoveImageOptions,
};
use futures::StreamExt;
use tracing::instrument;

use crate::backend::{Vm, VmBackend};
use crate::types::{OutputSink, SnapshotId, SnapshotLabel, VmConfig, WorkspaceMount};

/// Docker-based VM backend.
///
/// Each VM is a long-lived container; snapshots are committed images.
#[derive(Debug)]
pub struct DockerBackend {
    client: Docker,
}

impl DockerBackend {
    /// Connect to the local Docker daemon.
    ///
    /// # Errors
    ///
    /// Returns an error if bollard cannot resolve a Docker endpoint.
    pub fn connect() -> Result<Self> {
        let client =
            Docker::connect_with_local_defaults().context("failed to connect to Docker daemon")?;
        Ok(Self { client })
    }

    #[instrument(skip(self))]
    async fn ensure_image(&self, image: &str) -> Result<()> {
        if self.image_exists_by_tag(image).await? {
            return Ok(());
        }
        let mut stream = self.client.create_image(
            Some(CreateImageOptions {
                from_image: image,
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(item) = stream.next().await {
            item.with_context(|| format!("pulling image '{image}'"))?;
        }
        Ok(())
    }

    /// Check whether an image with the given tag exists locally.
    async fn image_exists_by_tag(&self, tag: &str) -> Result<bool> {
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![tag.to_string()]);
        let images = self
            .client
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await
            .with_context(|| format!("listing images for tag '{tag}'"))?;
        Ok(!images.is_empty())
    }

    /// Compute the host-ownership reclaim spec for a bind-mounted workspace.
    ///
    /// Steps run as the image's default user (typically root) and write
    /// straight into the bind-mounted host directory, so on native Linux
    /// Docker the files land on the host owned by uid 0. The unprivileged
    /// `hm` user then can neither COW-copy 0600/0700 entries into child
    /// workspaces nor delete them during cleanup. The reclaim spec records
    /// the owner of the host tempdir itself (always the `hm` user that
    /// created it) so [`reclaim_workspace_ownership`] can chown everything
    /// back before the host touches the directory again.
    #[cfg(unix)]
    fn reclaim_spec(workspace: Option<&WorkspaceMount>) -> Option<WorkspaceReclaim> {
        use std::os::unix::fs::MetadataExt;
        let ws = workspace?;
        let meta = std::fs::metadata(&ws.host_path).ok()?;
        Some(WorkspaceReclaim {
            guest_path: ws.guest_path.clone(),
            uid: meta.uid(),
            gid: meta.gid(),
        })
    }

    #[cfg(not(unix))]
    fn reclaim_spec(_workspace: Option<&WorkspaceMount>) -> Option<WorkspaceReclaim> {
        None
    }

    #[instrument(skip(self))]
    async fn start_container(
        &self,
        image: &str,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<String> {
        let host_config = workspace.map(|ws| bollard::service::HostConfig {
            binds: Some(vec![format!(
                "{}:{}:rw",
                ws.host_path.display(),
                ws.guest_path
            )]),
            ..Default::default()
        });
        let cfg = Config {
            image: Some(image.to_string()),
            cmd: Some(vec!["sh".into(), "-c".into(), "sleep infinity".into()]),
            host_config,
            ..Default::default()
        };
        let create = self
            .client
            .create_container(None::<CreateContainerOptions<String>>, cfg)
            .await
            .context("create container")?;
        if let Err(e) = self
            .client
            .start_container(&create.id, None::<StartContainerOptions<String>>)
            .await
        {
            // The daemon validates bind-mount sources at START time (e.g.
            // Docker Desktop's file-sharing allowlist), so this path is
            // reachable on every step. No `Vm` handle exists yet — neither
            // `HmVm::execute`'s `destroy()` nor `DockerVm`'s `Drop` backstop
            // will ever see this container, and image GC cannot reclaim an
            // image pinned by a Created-state container — so remove it here,
            // best-effort, before propagating the error.
            if let Err(rm_err) = self
                .client
                .remove_container(
                    &create.id,
                    Some(RemoveContainerOptions {
                        force: true,
                        v: true,
                        ..Default::default()
                    }),
                )
                .await
            {
                tracing::warn!(
                    container = %create.id,
                    error = %rm_err,
                    "failed to remove container that never started"
                );
            }
            return Err(anyhow::Error::new(e).context("start container"));
        }
        Ok(create.id)
    }
}

#[async_trait]
impl VmBackend for DockerBackend {
    #[instrument(skip(self, _config, workspace))]
    async fn create(
        &self,
        image: &str,
        _config: &VmConfig,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<Box<dyn Vm>> {
        self.ensure_image(image).await?;
        let container_id = self.start_container(image, workspace).await?;
        Ok(Box::new(DockerVm {
            client: self.client.clone(),
            container_id: Some(container_id),
            workspace_reclaim: Self::reclaim_spec(workspace),
            exec_in_flight: AtomicBool::new(false),
        }))
    }

    #[instrument(skip(self, _config, workspace))]
    async fn restore(
        &self,
        snapshot: &SnapshotId,
        _config: &VmConfig,
        workspace: Option<&WorkspaceMount>,
    ) -> Result<Box<dyn Vm>> {
        let container_id = self.start_container(snapshot.as_ref(), workspace).await?;
        Ok(Box::new(DockerVm {
            client: self.client.clone(),
            container_id: Some(container_id),
            workspace_reclaim: Self::reclaim_spec(workspace),
            exec_in_flight: AtomicBool::new(false),
        }))
    }

    #[instrument(skip(self))]
    async fn snapshot_exists(&self, snapshot: &SnapshotId) -> Result<bool> {
        self.image_exists_by_tag(snapshot.as_ref()).await
    }

    #[instrument(skip(self))]
    async fn remove_snapshot(&self, snapshot: &SnapshotId) -> Result<()> {
        self.client
            .remove_image(
                snapshot.as_ref(),
                Some(RemoveImageOptions {
                    force: true,
                    noprune: false,
                }),
                None,
            )
            .await
            .with_context(|| format!("removing image '{snapshot}'"))?;
        Ok(())
    }

    #[instrument(skip(self, keep))]
    async fn gc_snapshots(
        &self,
        reference: &str,
        older_than: std::time::Duration,
        keep: &(dyn for<'a> Fn(&'a str) -> bool + Send + Sync),
    ) -> Result<u64> {
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![reference.to_string()]);
        let images = self
            .client
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await
            .with_context(|| format!("listing images for GC reference '{reference}'"))?;

        let cutoff: i64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .saturating_sub(older_than)
            .as_secs()
            .try_into()
            .unwrap_or(i64::MAX);

        let mut removed: u64 = 0;
        for image in images {
            if image.created >= cutoff {
                continue;
            }
            // Remove by tag, never by image id: the daemon's `reference`
            // filter returns whole images, and a multi-tag image may carry
            // tags outside the GC pattern (or tags the keeper protects).
            // Untagging only the matched, unprotected references leaves
            // every other tag pointing at the image intact; the image data
            // itself is reclaimed when its last tag goes.
            for tag in &image.repo_tags {
                if !reference_matches(reference, tag) {
                    continue;
                }
                if keep(tag) {
                    tracing::debug!(image = %tag, "GC keeping referenced snapshot");
                    continue;
                }
                match self
                    .client
                    .remove_image(
                        tag,
                        Some(RemoveImageOptions {
                            force: true,
                            noprune: false,
                        }),
                        None,
                    )
                    .await
                {
                    Ok(_) => removed += 1,
                    Err(e) => {
                        tracing::warn!(image = %tag, error = %e, "failed to GC snapshot image");
                    }
                }
            }
        }
        Ok(removed)
    }
}

/// Client-side check that a `repo:tag` reference belongs to a GC pattern.
///
/// The daemon's `reference` filter pre-selects *images*, but a multi-tag
/// image's `repo_tags` can include tags outside the pattern; only matching
/// tags may be untagged. Supports the two pattern shapes the GC uses:
///
/// - an exact repository name (matches every tag of exactly that repo);
/// - `<repo>/*` (matches repositories exactly one path component below
///   `<repo>`, mirroring Docker's reference-filter glob semantics).
fn reference_matches(pattern: &str, repo_tag: &str) -> bool {
    let repo = repo_tag.rsplit_once(':').map_or(repo_tag, |(r, _)| r);
    pattern.strip_suffix("/*").map_or_else(
        || repo == pattern,
        |prefix| {
            repo.strip_prefix(prefix).is_some_and(|rest| {
                rest.strip_prefix('/')
                    .is_some_and(|name| !name.is_empty() && !name.contains('/'))
            })
        },
    )
}

/// Ownership-reclaim parameters for a bind-mounted workspace: chown the
/// guest path back to the host user that owns the workspace tempdir.
#[derive(Debug, Clone)]
struct WorkspaceReclaim {
    guest_path: String,
    uid: u32,
    gid: u32,
}

/// Best-effort: chown the bind-mounted workspace back to the host user.
///
/// Runs `chown -R <uid>:<gid> <guest_path>` inside the container as root
/// (regardless of the image's default user). Because a bind mount shares
/// inodes with the host, this restores host-side ownership of every file
/// the step wrote as root, so the unprivileged host user can COW-copy the
/// workspace into children and delete it during cleanup. On Docker Desktop
/// (macOS) ownership is already remapped by the file sharing layer and
/// this is a harmless no-op.
///
/// Failures are logged, never propagated: a missing `chown` binary or a
/// dead container must not fail the build — host-side cleanup will then
/// warn about anything it cannot remove.
async fn reclaim_workspace_ownership(client: &Docker, container_id: &str, spec: &WorkspaceReclaim) {
    let owner = format!("{}:{}", spec.uid, spec.gid);
    let exec = match client
        .create_exec(
            container_id,
            CreateExecOptions {
                cmd: Some(vec!["chown", "-R", &owner, &spec.guest_path]),
                user: Some("0:0"),
                attach_stdout: Some(true),
                attach_stderr: Some(true),
                ..Default::default()
            },
        )
        .await
    {
        Ok(exec) => exec,
        Err(e) => {
            tracing::warn!(container = %container_id, error = %e,
                "failed to create workspace ownership-reclaim exec");
            return;
        }
    };

    match client.start_exec(&exec.id, None).await {
        Ok(StartExecResults::Attached { mut output, .. }) => {
            // Drain so the exec runs to completion.
            while let Some(item) = output.next().await {
                if item.is_err() {
                    break;
                }
            }
        }
        Ok(StartExecResults::Detached) => {}
        Err(e) => {
            tracing::warn!(container = %container_id, error = %e,
                "failed to start workspace ownership-reclaim exec");
            return;
        }
    }

    match client.inspect_exec(&exec.id).await {
        Ok(inspect) if inspect.exit_code.unwrap_or(0) == 0 => {}
        Ok(inspect) => {
            tracing::warn!(container = %container_id, exit_code = ?inspect.exit_code,
                "workspace ownership reclaim (chown) exited non-zero; host-side cleanup may be incomplete");
        }
        Err(e) => {
            tracing::warn!(container = %container_id, error = %e,
                "failed to inspect workspace ownership-reclaim exec");
        }
    }
}

/// Handle to a running Docker container acting as a VM.
#[derive(derive_more::Debug)]
struct DockerVm {
    #[debug(skip)]
    client: Docker,
    container_id: Option<String>,
    /// When a workspace is bind-mounted, the chown target used to hand
    /// root-written files back to the host user before teardown.
    workspace_reclaim: Option<WorkspaceReclaim>,
    /// True while a command may still be running inside the container.
    ///
    /// Set when `exec` starts and cleared only when it completes cleanly,
    /// so a cancelled/timed-out (dropped) exec future — or a stream error
    /// of unknown outcome — leaves it set. Teardown uses it to quiesce the
    /// container (SIGKILL every process, then restart the idle `sleep`)
    /// before the ownership-reclaim chown, so no in-container writer can
    /// race the chown or dirty the bind mount afterwards.
    exec_in_flight: AtomicBool,
}

/// Shared container teardown used by both [`DockerVm::destroy`] (awaited)
/// and [`DockerVm::drop`] (detached backstop).
///
/// Order matters: when `exec_in_flight` is set, a cancelled or timed-out
/// command may still be writing into the bind mount as root, so the
/// container is stopped first (SIGKILL, quiescing every process) and then
/// restarted (its command is `sleep infinity`) so the reclaim chown runs
/// with no concurrent writers. Only after the chown does the final
/// stop/remove happen — nothing can re-dirty the workspace between the
/// chown and the host-side removal of the directory.
async fn teardown_container(
    client: &Docker,
    id: &str,
    reclaim: Option<WorkspaceReclaim>,
    exec_in_flight: bool,
) -> Result<()> {
    if let Some(spec) = reclaim {
        let mut can_reclaim = true;
        if exec_in_flight {
            let _ = client
                .stop_container(id, Some(StopContainerOptions { t: 0 }))
                .await;
            if let Err(e) = client
                .start_container(id, None::<StartContainerOptions<String>>)
                .await
            {
                tracing::warn!(container = %id, error = %e,
                    "failed to restart container for workspace ownership reclaim; \
                     host-side workspace cleanup may be incomplete");
                can_reclaim = false;
            }
        }
        if can_reclaim {
            reclaim_workspace_ownership(client, id, &spec).await;
        }
    }
    let _ = client
        .stop_container(id, Some(StopContainerOptions { t: 0 }))
        .await;
    client
        .remove_container(
            id,
            Some(RemoveContainerOptions {
                force: true,
                v: true,
                ..Default::default()
            }),
        )
        .await
        .with_context(|| format!("removing container '{id}'"))?;
    Ok(())
}

impl Drop for DockerVm {
    fn drop(&mut self) {
        if let Some(id) = self.container_id.take() {
            let client = self.client.clone();
            let reclaim = self.workspace_reclaim.take();
            let exec_in_flight = self.exec_in_flight.load(Ordering::Acquire);
            tokio::spawn(async move {
                match teardown_container(&client, &id, reclaim, exec_in_flight).await {
                    Ok(()) => tracing::debug!(container = %id, "dropped container cleaned up"),
                    Err(e) => {
                        tracing::warn!(container = %id, error = %e, "dropped container cleanup failed");
                    }
                }
            });
        }
    }
}

#[async_trait]
impl Vm for DockerVm {
    #[instrument(skip(self, env, sink))]
    async fn exec(
        &self,
        cmd: &str,
        env: &[(String, String)],
        working_dir: &str,
        sink: &dyn OutputSink,
    ) -> Result<i32> {
        let cid = self
            .container_id
            .as_deref()
            .context("container already destroyed")?;
        // Mark the container as possibly-running-a-command until we have
        // proof of completion. Cleared only on the clean-return path below;
        // a dropped (cancelled/timed-out) future or a stream error leaves
        // it set so teardown quiesces the container before the workspace
        // ownership reclaim.
        self.exec_in_flight.store(true, Ordering::Release);
        let env_strings: Vec<String> = env.iter().map(|(k, v)| format!("{k}={v}")).collect();
        let exec = self
            .client
            .create_exec(
                cid,
                CreateExecOptions {
                    cmd: Some(vec!["sh", "-c", cmd]),
                    env: Some(env_strings.iter().map(String::as_str).collect()),
                    working_dir: Some(working_dir),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .context("create exec")?;

        if let StartExecResults::Attached { mut output, .. } = self
            .client
            .start_exec(&exec.id, None)
            .await
            .context("start exec")?
        {
            use bollard::container::LogOutput;

            while let Some(item) = output.next().await {
                let chunk = item.context("exec stream")?;
                match chunk {
                    LogOutput::StdOut { message } => {
                        let text = String::from_utf8_lossy(&message);
                        for line in text.lines() {
                            sink.on_stdout(line);
                        }
                    }
                    LogOutput::StdErr { message } => {
                        let text = String::from_utf8_lossy(&message);
                        for line in text.lines() {
                            sink.on_stderr(line);
                        }
                    }
                    LogOutput::StdIn { .. } | LogOutput::Console { .. } => {}
                }
            }
        }

        // Retry inspect_exec: the connection pool can go stale after
        // long-running exec streams on Docker Desktop for macOS.
        let mut inspect_result = self.client.inspect_exec(&exec.id).await;
        for _ in 0..3 {
            if inspect_result.is_ok() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
            inspect_result = self.client.inspect_exec(&exec.id).await;
        }
        let inspect = inspect_result.context("inspect exec")?;

        #[allow(
            clippy::cast_possible_truncation,
            reason = "docker exit codes fit in i32"
        )]
        let exit_code = inspect.exit_code.unwrap_or(0) as i32;
        // The command finished and its exit code was observed: no process
        // of ours is writing to the bind mount any more.
        self.exec_in_flight.store(false, Ordering::Release);
        Ok(exit_code)
    }

    #[instrument(skip(self))]
    async fn snapshot(&mut self, label: &SnapshotLabel) -> Result<SnapshotId> {
        let cid = self
            .container_id
            .as_deref()
            .context("container already destroyed")?;
        // An ephemeral, uncached snapshot is committed under a unique tag (the
        // container id) rather than a shared `:latest`: concurrent sibling leaf
        // steps off the same parent all commit ephemeral snapshots, and racing
        // to write the same `ephemeral:latest` image fails the loser of the
        // race in dockerd. A cached snapshot parses its cache key as `repo:tag`.
        let (repo, tag) = match label {
            SnapshotLabel::Ephemeral => ("ephemeral", cid),
            SnapshotLabel::Cached(key) => match key.split_once(':') {
                Some((r, v)) => (r, v),
                None => (key.as_str(), cid),
            },
        };
        let opts = CommitContainerOptions {
            container: cid,
            repo,
            tag,
            ..Default::default()
        };
        // docker commit can be slow for containers with large filesystems;
        // use a dedicated long-timeout client for this operation.
        #[allow(
            clippy::duration_suboptimal_units,
            reason = "from_mins is nightly-only"
        )]
        let commit_client = self
            .client
            .clone()
            .with_timeout(std::time::Duration::from_secs(600));
        commit_client
            .commit_container(opts, Config::<String>::default())
            .await
            .context("commit container")?;
        let full_tag = format!("{repo}:{tag}");
        Ok(SnapshotId::new(full_tag))
    }

    #[instrument(skip(self))]
    async fn destroy(&mut self) -> Result<()> {
        let Some(id) = self.container_id.take() else {
            return Ok(());
        };
        // Hand root-written workspace files back to the host user before
        // teardown: `HmVm::execute` always destroys the VM (awaited, even
        // on cancellation) before the runner keeps or drops the tempdir,
        // so the reclaim happens-before every host-side read or removal of
        // the workspace. When the command was cut short (`exec_in_flight`
        // still set) the container is quiesced first so no in-container
        // writer can race or follow the chown.
        let reclaim = self.workspace_reclaim.take();
        let exec_in_flight = self.exec_in_flight.load(Ordering::Acquire);
        teardown_container(&self.client, &id, reclaim, exec_in_flight).await
    }
}

#[cfg(test)]
mod tests {
    use super::reference_matches;

    #[test]
    fn exact_repo_pattern_matches_every_tag_of_that_repo() {
        assert!(reference_matches("ephemeral", "ephemeral:latest"));
        assert!(reference_matches(
            "ephemeral",
            "ephemeral:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcd"
        ));
        assert!(reference_matches(
            "harmont-ephemeral",
            "harmont-ephemeral:3c5e0fda-1111-2222-3333-444444444444"
        ));
    }

    #[test]
    fn exact_repo_pattern_rejects_other_repos() {
        // A user's own image whose repo merely starts with the pattern
        // must never be swept.
        assert!(!reference_matches("ephemeral", "ephemeral-test:latest"));
        assert!(!reference_matches("ephemeral", "my-ephemeral:latest"));
        assert!(!reference_matches("ephemeral", "ephemeral/sub:latest"));
    }

    #[test]
    fn wildcard_pattern_matches_one_path_component() {
        assert!(reference_matches(
            "harmont-cache/*",
            "harmont-cache/build:0123456789abcdef"
        ));
        assert!(!reference_matches(
            "harmont-cache/*",
            "harmont-cache:latest"
        ));
        assert!(!reference_matches(
            "harmont-cache/*",
            "harmont-cachex/build:latest"
        ));
        assert!(!reference_matches(
            "harmont-cache/*",
            "harmont-cache/a/b:latest"
        ));
        assert!(!reference_matches("harmont-cache/*", "other/build:latest"));
    }
}
