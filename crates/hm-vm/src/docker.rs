//! Docker backend -- container orchestration via bollard.
//!
//! Each "VM" is a long-lived container running `sleep infinity`,
//! commands are executed via the exec API, and snapshots are Docker
//! image commits.

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions, UploadToContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::{
    CommitContainerOptions, CreateImageOptions, ListImagesOptions, RemoveImageOptions,
};
use futures::StreamExt;
use tracing::instrument;

use crate::backend::{Vm, VmBackend};
use crate::types::{OutputSink, SnapshotId, SnapshotLabel, VmConfig};

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

    #[instrument(skip(self))]
    async fn start_container(&self, image: &str) -> Result<String> {
        let cfg = Config {
            image: Some(image.to_string()),
            cmd: Some(vec!["sh".into(), "-c".into(), "sleep infinity".into()]),
            ..Default::default()
        };
        let create = self
            .client
            .create_container(None::<CreateContainerOptions<String>>, cfg)
            .await
            .context("create container")?;
        self.client
            .start_container(&create.id, None::<StartContainerOptions<String>>)
            .await
            .context("start container")?;
        Ok(create.id)
    }
}

#[async_trait]
impl VmBackend for DockerBackend {
    #[instrument(skip(self, _config))]
    async fn create(&self, image: &str, _config: &VmConfig) -> Result<Box<dyn Vm>> {
        self.ensure_image(image).await?;
        let container_id = self.start_container(image).await?;
        Ok(Box::new(DockerVm {
            client: self.client.clone(),
            container_id: Some(container_id),
        }))
    }

    #[instrument(skip(self, _config))]
    async fn restore(&self, snapshot: &SnapshotId, _config: &VmConfig) -> Result<Box<dyn Vm>> {
        let container_id = self.start_container(snapshot.as_ref()).await?;
        Ok(Box::new(DockerVm {
            client: self.client.clone(),
            container_id: Some(container_id),
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
}

/// Handle to a running Docker container acting as a VM.
#[derive(derive_more::Debug)]
struct DockerVm {
    #[debug(skip)]
    client: Docker,
    container_id: Option<String>,
}

impl Drop for DockerVm {
    fn drop(&mut self) {
        if let Some(id) = self.container_id.take() {
            let client = self.client.clone();
            tokio::spawn(async move {
                let opts = StopContainerOptions { t: 0 };
                let _ = client.stop_container(&id, Some(opts)).await;
                let rm = RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                };
                let _ = client.remove_container(&id, Some(rm)).await;
                tracing::debug!(container = %id, "dropped container cleaned up");
            });
        }
    }
}

/// Build a tar archive from a host directory.
///
/// The archive contains all files under `host_path` with paths relative
/// to `host_path` itself (i.e. the directory contents, not the directory).
fn tar_directory(host_path: &Path) -> Result<Vec<u8>> {
    let mut archive = tar::Builder::new(Vec::new());
    archive
        .append_dir_all(".", host_path)
        .with_context(|| format!("archiving '{}'", host_path.display()))?;
    archive.finish().context("finalizing tar archive")?;
    archive.into_inner().context("extracting tar bytes")
}

#[async_trait]
impl Vm for DockerVm {
    #[instrument(skip(self), fields(host = %host_path.display()))]
    async fn inject(&self, host_path: &Path, guest_path: &str) -> Result<()> {
        // Ensure the destination directory exists inside the container.
        let cid = self
            .container_id
            .as_deref()
            .context("container already destroyed")?;
        let mkdir = self
            .client
            .create_exec(
                cid,
                CreateExecOptions {
                    cmd: Some(vec!["mkdir", "-p", guest_path]),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .context("create mkdir exec")?;
        if let StartExecResults::Attached { mut output, .. } = self
            .client
            .start_exec(&mkdir.id, None)
            .await
            .context("start mkdir exec")?
        {
            while output.next().await.is_some() {}
        }

        let tar_bytes = tar_directory(host_path)?;
        let options = UploadToContainerOptions {
            path: guest_path,
            ..Default::default()
        };
        self.client
            .upload_to_container(cid, Some(options), tar_bytes.into())
            .await
            .with_context(|| {
                format!(
                    "uploading '{}' to container '{}:{guest_path}'",
                    host_path.display(),
                    cid.get(..12).unwrap_or(cid),
                )
            })?;
        Ok(())
    }

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
        let _ = self
            .client
            .stop_container(&id, Some(StopContainerOptions { t: 0 }))
            .await;
        self.client
            .remove_container(
                &id,
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
}
