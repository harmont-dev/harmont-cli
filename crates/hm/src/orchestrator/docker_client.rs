//! Thin wrapper around bollard for the local executor.
//!
//! Operations: pull images, start containers (long-lived sleep), exec
//! commands streaming stdout/stderr, commit container to image, look
//! up images by tag, stop and remove containers.

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::{Context, Result};
use bollard::Docker;
use bollard::container::{
    Config, CreateContainerOptions, RemoveContainerOptions, StartContainerOptions,
    StopContainerOptions,
};
use bollard::exec::{CreateExecOptions, StartExecResults};
use bollard::image::{
    CommitContainerOptions, CreateImageOptions, ImportImageOptions, ListImagesOptions,
    RemoveImageOptions,
};
use bollard::models::HostConfig;
use futures_util::StreamExt;
use tokio::io::AsyncWrite;

use crate::error::HmError;

/// Build a [`HostConfig`] with optional bind mounts and Linux capabilities.
///
/// Empty slices become `None` so Docker applies its defaults.
fn build_host_config(binds: &[String], cap_add: &[String]) -> HostConfig {
    HostConfig {
        binds: if binds.is_empty() {
            None
        } else {
            Some(binds.to_vec())
        },
        cap_add: if cap_add.is_empty() {
            None
        } else {
            Some(cap_add.to_vec())
        },
        ..Default::default()
    }
}

#[derive(Debug, Clone)]
pub struct DockerClient {
    inner: Arc<Docker>,
}

impl DockerClient {
    /// Open a Docker connection using the platform's default socket /
    /// pipe. The handle is cheap to clone (refcounted internally).
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] when bollard cannot resolve a
    /// local Docker endpoint (no socket on `DOCKER_HOST`, no Windows
    /// pipe, etc.).
    pub fn connect() -> Result<Self> {
        let d = Docker::connect_with_local_defaults()
            .map_err(|e| HmError::Docker(format!("connect: {e}")))?;
        Ok(Self { inner: Arc::new(d) })
    }

    /// Round-trip the daemon to confirm reachability.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the ping request fails (daemon
    /// stopped, socket revoked, version negotiation failure).
    pub async fn ping(&self) -> Result<()> {
        self.inner
            .ping()
            .await
            .map_err(|e| HmError::Docker(format!("ping failed: {e}")))?;
        Ok(())
    }

    /// True if `tag` resolves to a locally-cached image.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the `list_images` API call
    /// fails (daemon unreachable, malformed filter).
    pub async fn image_exists(&self, tag: &str) -> Result<bool> {
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![tag.to_string()]);
        let images = self
            .inner
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|e| HmError::Docker(format!("list_images: {e}")))?;
        Ok(!images.is_empty())
    }

    /// List all `repo_tags` from images that have at least one tag
    /// matching `reference` (e.g. `"harmont-local/build"` matches
    /// `harmont-local/build:abc123`).
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the `list_images` API call fails.
    pub async fn list_images_by_reference(&self, reference: &str) -> Result<Vec<String>> {
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![format!("{reference}:*")]);
        let images = self
            .inner
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|e| HmError::Docker(format!("list_images: {e}")))?;
        Ok(images.into_iter().flat_map(|img| img.repo_tags).collect())
    }

    /// Pull `tag` from its registry, surfacing the daemon's progress
    /// stream as Docker errors.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if any chunk of the pull stream
    /// reports an error (registry not reachable, image not found,
    /// auth required).
    pub async fn pull_image(&self, tag: &str) -> Result<()> {
        let mut s = self.inner.create_image(
            Some(CreateImageOptions {
                from_image: tag,
                ..Default::default()
            }),
            None,
            None,
        );
        while let Some(item) = s.next().await {
            item.map_err(|e| HmError::Docker(format!("pull {tag}: {e}")))?;
        }
        Ok(())
    }

    /// Start a long-lived container that runs `sh -c 'sleep infinity'` so
    /// later `exec`s land in a stable shell. Returns the container ID.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the container cannot be created
    /// (image not pulled, name conflict, OCI runtime failure) or if
    /// `start_container` rejects the create.
    pub async fn start_long_lived(
        &self,
        image: &str,
        env: &[String],
        workdir: &str,
        name: &str,
    ) -> Result<String> {
        self.start_long_lived_with_mounts(image, env, workdir, name, &[])
            .await
    }

    /// Like [`Self::start_long_lived`] but with bind mounts via `HostConfig`.
    ///
    /// Each entry in `binds` is a Docker bind-mount string of the form
    /// `"/host/path:/container/path"` (with an optional `:ro` suffix).
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the container cannot be created
    /// (image not pulled, name conflict, OCI runtime failure) or if
    /// `start_container` rejects the create.
    pub async fn start_long_lived_with_mounts(
        &self,
        image: &str,
        env: &[String],
        workdir: &str,
        name: &str,
        binds: &[String],
    ) -> Result<String> {
        let cfg = Config {
            image: Some(image.to_string()),
            cmd: Some(vec!["sh".into(), "-c".into(), "sleep infinity".into()]),
            env: Some(env.to_vec()),
            working_dir: Some(workdir.to_string()),
            host_config: Some(build_host_config(binds, &[])),
            ..Default::default()
        };
        let create = self
            .inner
            .create_container(
                Some(CreateContainerOptions {
                    name,
                    ..Default::default()
                }),
                cfg,
            )
            .await
            .map_err(|e| HmError::Docker(format!("create_container: {e}")))?;
        self.inner
            .start_container(&create.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| HmError::Docker(format!("start_container: {e}")))?;
        Ok(create.id)
    }

    /// Exec a command inside a running container and stream stdout+stderr
    /// to `out`. Returns the command's exit code.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if `create_exec` / `start_exec` /
    /// `inspect_exec` fail, or surfaces an `anyhow` error if writing a
    /// log frame to `out` fails.
    pub async fn exec_streaming(
        &self,
        container_id: &str,
        cmd: &[String],
        env: &[String],
        workdir: &str,
        out: &mut (impl AsyncWrite + Send + Unpin),
    ) -> Result<i64> {
        use bollard::container::LogOutput;
        use tokio::io::AsyncWriteExt;

        let exec = self
            .inner
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(cmd.iter().map(std::string::String::as_str).collect()),
                    env: Some(env.iter().map(std::string::String::as_str).collect()),
                    working_dir: Some(workdir),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| HmError::Docker(format!("create_exec: {e}")))?;
        match self
            .inner
            .start_exec(&exec.id, None)
            .await
            .map_err(|e| HmError::Docker(format!("start_exec: {e}")))?
        {
            StartExecResults::Attached { mut output, .. } => {
                while let Some(item) = output.next().await {
                    let chunk = item.map_err(|e| HmError::Docker(format!("exec stream: {e}")))?;
                    let (LogOutput::StdOut { message: bytes }
                    | LogOutput::StdErr { message: bytes }
                    | LogOutput::Console { message: bytes }) = chunk
                    else {
                        // StdIn frames are echoed by some daemons; ignore them.
                        continue;
                    };
                    out.write_all(&bytes).await.context("write exec output")?;
                }
            }
            StartExecResults::Detached => {}
        }
        let inspect = self
            .inner
            .inspect_exec(&exec.id)
            .await
            .map_err(|e| HmError::Docker(format!("inspect_exec: {e}")))?;
        Ok(inspect.exit_code.unwrap_or(0))
    }

    /// Like [`Self::exec_streaming`], but also pipes `stdin_bytes` into the
    /// exec'd process's stdin (closing it after the write so the process
    /// sees EOF). Used to stream a tar archive into `tar -xzf -` when
    /// hydrating `/workspace` in a fresh chain-root container.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if any of the exec lifecycle calls
    /// fail, or surfaces an `anyhow` error if writing stdin or output
    /// frames fails.
    pub async fn exec_streaming_stdin(
        &self,
        container_id: &str,
        cmd: &[String],
        env: &[String],
        workdir: &str,
        stdin_bytes: &[u8],
        out: &mut (impl AsyncWrite + Send + Unpin),
    ) -> Result<i64> {
        use bollard::container::LogOutput;
        use tokio::io::AsyncWriteExt;

        let exec = self
            .inner
            .create_exec(
                container_id,
                CreateExecOptions {
                    cmd: Some(cmd.iter().map(std::string::String::as_str).collect()),
                    env: Some(env.iter().map(std::string::String::as_str).collect()),
                    working_dir: Some(workdir),
                    attach_stdin: Some(true),
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| HmError::Docker(format!("create_exec: {e}")))?;
        match self
            .inner
            .start_exec(&exec.id, None)
            .await
            .map_err(|e| HmError::Docker(format!("start_exec: {e}")))?
        {
            StartExecResults::Attached {
                mut output,
                mut input,
            } => {
                input
                    .write_all(stdin_bytes)
                    .await
                    .context("write exec stdin")?;
                input.shutdown().await.context("close exec stdin")?;
                // Drop the writer to fully release the half-duplex.
                drop(input);
                while let Some(item) = output.next().await {
                    let chunk = item.map_err(|e| HmError::Docker(format!("exec stream: {e}")))?;
                    let (LogOutput::StdOut { message: bytes }
                    | LogOutput::StdErr { message: bytes }
                    | LogOutput::Console { message: bytes }) = chunk
                    else {
                        // StdIn frames are echoed by some daemons; ignore them.
                        continue;
                    };
                    out.write_all(&bytes).await.context("write exec output")?;
                }
            }
            StartExecResults::Detached => {}
        }
        let inspect = self
            .inner
            .inspect_exec(&exec.id)
            .await
            .map_err(|e| HmError::Docker(format!("inspect_exec: {e}")))?;
        Ok(inspect.exit_code.unwrap_or(0))
    }

    /// Commit a running container to an image tag. Returns the tag, which
    /// is a valid image reference once the daemon's commit succeeds.
    ///
    /// We don't return the daemon's image ID: bollard 0.18's `Commit`
    /// stub deserialises the response as `{"id": ...}`, but the Docker
    /// daemon returns `{"Id": ...}` (capital I). The image is committed
    /// correctly either way; the tag is the canonical reference and is
    /// what every caller actually uses.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if `commit_container` fails (paused
    /// container, daemon I/O failure).
    ///
    /// # Panics
    ///
    /// Panics if `tag.splitn(2, ':')` produces neither one nor two parts.
    /// `splitn` is total for non-empty input, so this branch is only
    /// reachable for the empty string, which the caller never passes.
    pub async fn commit_container(&self, container_id: &str, tag: &str) -> Result<String> {
        let parts: Vec<&str> = tag.splitn(2, ':').collect();
        let (repo, ver) = match parts.as_slice() {
            [r, v] => (*r, *v),
            [r] => (*r, "latest"),
            _ => unreachable!("splitn(2) yields one or two parts for non-empty input"),
        };
        let opts = CommitContainerOptions {
            container: container_id,
            repo,
            tag: ver,
            ..Default::default()
        };
        self.inner
            .commit_container(opts, Config::<String>::default())
            .await
            .map_err(|e| HmError::Docker(format!("commit_container: {e}")))?;
        Ok(tag.to_string())
    }

    /// Force-remove an image by tag. Used for end-of-run pruning of
    /// ephemeral parent-snapshot tags committed during this process's
    /// run. Best-effort callers should swallow the error themselves;
    /// failures here are non-fatal.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if `remove_image` fails (image
    /// missing, still referenced by a running container, daemon I/O
    /// failure).
    pub async fn remove_image(&self, image: &str) -> Result<()> {
        self.inner
            .remove_image(
                image,
                Some(RemoveImageOptions {
                    force: true,
                    noprune: false,
                }),
                None,
            )
            .await
            .map_err(|e| HmError::Docker(format!("remove_image '{image}': {e}")))?;
        Ok(())
    }

    /// Export a Docker image to a tar file on disk.
    ///
    /// Streams the image layer data from the daemon and writes it to
    /// `dest` using a buffered writer.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the daemon's export stream fails,
    /// or an I/O error if writing to `dest` fails.
    pub async fn export_image(&self, image: &str, dest: &std::path::Path) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let mut stream = self.inner.export_image(image);
        let file = tokio::fs::File::create(dest)
            .await
            .with_context(|| format!("create export file '{}'", dest.display()))?;
        let mut writer = tokio::io::BufWriter::new(file);
        while let Some(chunk) = stream.next().await {
            let bytes =
                chunk.map_err(|e| HmError::Docker(format!("export_image '{image}': {e}")))?;
            writer
                .write_all(&bytes)
                .await
                .with_context(|| format!("write export data to '{}'", dest.display()))?;
        }
        writer
            .flush()
            .await
            .with_context(|| format!("flush export file '{}'", dest.display()))?;
        Ok(())
    }

    /// Import a Docker image from a tar file on disk.
    ///
    /// Reads the full tar file into memory and loads it into the
    /// daemon via the image import API.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the daemon rejects the import
    /// stream, or an I/O error if reading `src` fails.
    pub async fn import_image(&self, src: &std::path::Path) -> Result<()> {
        let body = tokio::fs::read(src)
            .await
            .with_context(|| format!("read import file '{}'", src.display()))?;
        let mut stream =
            self.inner
                .import_image(ImportImageOptions { quiet: true }, body.into(), None);
        while let Some(item) = stream.next().await {
            item.map_err(|e| HmError::Docker(format!("import_image '{}': {e}", src.display())))?;
        }
        Ok(())
    }

    /// List all image tags whose name starts with `prefix`.
    ///
    /// Uses the Docker `reference` filter with a glob pattern and then
    /// post-filters the returned `repo_tags` to those that truly begin
    /// with `prefix`. The result is sorted lexicographically.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the `list_images` API call
    /// fails (daemon unreachable, malformed filter).
    pub async fn list_images_by_prefix(&self, prefix: &str) -> Result<Vec<String>> {
        let mut filters = HashMap::new();
        filters.insert("reference".to_string(), vec![format!("{prefix}*")]);
        let images = self
            .inner
            .list_images(Some(ListImagesOptions {
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|e| HmError::Docker(format!("list_images: {e}")))?;
        let mut tags: Vec<String> = images
            .iter()
            .flat_map(|img| &img.repo_tags)
            .filter(|tag| tag.starts_with(prefix))
            .cloned()
            .collect();
        tags.sort();
        Ok(tags)
    }

    pub async fn stop_remove(&self, container_id: &str) {
        let _ = self
            .inner
            .stop_container(container_id, Some(StopContainerOptions { t: 0 }))
            .await;
        let _ = self
            .inner
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    v: true,
                    ..Default::default()
                }),
            )
            .await;
    }

    /// Internal access to the underlying bollard handle, for callers
    /// that need to call bollard APIs not yet wrapped here (e.g., log
    /// streaming via `Docker::logs`).
    ///
    /// Prefer adding a dedicated method to this type; only use this
    /// accessor when a one-off stream is needed outside the main
    /// `DockerClient` API surface.
    #[doc(hidden)]
    #[must_use]
    pub fn inner_for_logs(&self) -> &bollard::Docker {
        &self.inner
    }

    /// List container summaries filtered by a single label `k=v` predicate.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] when `list_containers` fails.
    pub async fn list_containers_by_label(
        &self,
        k: &str,
        v: &str,
    ) -> Result<Vec<bollard::secret::ContainerSummary>> {
        use bollard::container::ListContainersOptions;
        use std::collections::HashMap;
        let mut filters: HashMap<String, Vec<String>> = HashMap::new();
        filters.insert("label".to_string(), vec![format!("{k}={v}")]);
        let out = self
            .inner
            .list_containers(Some(ListContainersOptions {
                all: true,
                filters,
                ..Default::default()
            }))
            .await
            .map_err(|e| HmError::Docker(format!("list_containers: {e}")))?;
        Ok(out)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod smoke {
    use super::*;

    #[tokio::test]
    #[ignore = "requires a running Docker daemon; opt in with `cargo test -- --ignored`"]
    async fn docker_ping() {
        let c = DockerClient::connect().unwrap();
        c.ping().await.unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a running Docker daemon; opt in with `cargo test -- --ignored`"]
    async fn list_images_by_reference_returns_empty_for_nonexistent() {
        let c = DockerClient::connect().unwrap();
        let tags = c
            .list_images_by_reference("harmont-test-nonexistent")
            .await
            .unwrap();
        assert!(tags.is_empty());
    }

    #[test]
    fn build_host_config_with_binds_and_no_caps() {
        let hc = super::build_host_config(&["/host/path:/container/path".to_string()], &[]);
        assert_eq!(
            hc.binds.as_ref().unwrap(),
            &["/host/path:/container/path".to_string()]
        );
        assert!(hc.cap_add.is_none());
    }

    #[test]
    fn build_host_config_empty_binds_is_none() {
        let hc = super::build_host_config(&[], &[]);
        assert!(hc.binds.is_none());
        assert!(hc.cap_add.is_none());
    }
}
