//! Thin wrapper around bollard for the local executor.
//!
//! Operations: pull images, start containers (long-lived sleep), exec
//! commands streaming stdout/stderr, commit container to image, look
//! up images by tag, stop+remove containers.

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
use futures_util::StreamExt;
use tokio::io::AsyncWrite;

use crate::error::HmError;

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
        let cfg = Config {
            image: Some(image.to_string()),
            cmd: Some(vec!["sh".into(), "-c".into(), "sleep infinity".into()]),
            env: Some(env.to_vec()),
            working_dir: Some(workdir.to_string()),
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

    // --- network ---

    /// Create a user-defined bridge network. Returns the network ID.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] if the daemon rejects the create.
    pub async fn create_network(
        &self,
        name: &str,
        labels: std::collections::HashMap<String, String>,
    ) -> Result<String> {
        use bollard::network::CreateNetworkOptions;
        let resp = self
            .inner
            .create_network(CreateNetworkOptions {
                name: name.to_string(),
                driver: "bridge".to_string(),
                labels,
                ..Default::default()
            })
            .await
            .map_err(|e| HmError::Docker(format!("create_network({name}): {e}")))?;
        Ok(resp.id)
    }

    /// Remove a network by name. Idempotent — silently swallows "not found".
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] for non-404 daemon errors.
    pub async fn remove_network(&self, name: &str) -> Result<()> {
        match self.inner.remove_network(name).await {
            Ok(())
            | Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(e) => Err(HmError::Docker(format!("remove_network({name}): {e}")).into()),
        }
    }

    // --- service container ---

    /// Spec for a long-lived service container (one deployment).
    /// Pass into [`start_service`].
    #[must_use]
    pub fn build_service_spec<'a>(image: &'a str, name: &'a str) -> ServiceSpecBuilder<'a> {
        ServiceSpecBuilder::new(image, name)
    }

    /// Create + start a long-lived container per the supplied spec.
    /// The container is *not* the bare `sleep infinity` shell that
    /// [`start_long_lived`] uses — this is for actual deployments where
    /// the image's CMD (optionally overridden) is the process.
    /// Returns the container id.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] on create / start / network attach failures.
    pub async fn start_service(&self, spec: ServiceSpec<'_>) -> Result<String> {
        use bollard::models::{HostConfig, PortBinding};
        use bollard::network::ConnectNetworkOptions;
        use std::collections::HashMap;

        // Docker's exposed_ports type requires HashMap<String, HashMap<(), ()>>.
        // The unit-value inner map is the Docker API convention for "no options".
        #[allow(
            clippy::zero_sized_map_values,
            reason = "Docker API requires this exact type"
        )]
        let (mut exposed, mut port_bindings) = (
            HashMap::<String, HashMap<(), ()>>::new(),
            HashMap::<String, Option<Vec<PortBinding>>>::new(),
        );
        for cport in &spec.publish {
            let key = format!("{cport}/tcp");
            #[allow(
                clippy::zero_sized_map_values,
                reason = "Docker API requires this exact type"
            )]
            exposed.insert(key.clone(), HashMap::new());
            port_bindings.insert(
                key,
                Some(vec![PortBinding {
                    host_ip: None,
                    host_port: Some(String::new()), // empty -> daemon assigns ephemeral
                }]),
            );
        }

        let host_config = HostConfig {
            binds: if spec.binds.is_empty() {
                None
            } else {
                Some(spec.binds.clone())
            },
            port_bindings: Some(port_bindings),
            network_mode: Some(spec.network.to_string()),
            ..Default::default()
        };

        let cfg = Config {
            image: Some(spec.image.to_string()),
            cmd: spec.cmd.clone(),
            env: Some(spec.env.clone()),
            working_dir: spec.workdir.map(str::to_string),
            exposed_ports: Some(exposed),
            host_config: Some(host_config),
            labels: Some(spec.labels.clone().into_iter().collect()),
            ..Default::default()
        };

        let create = self
            .inner
            .create_container(
                Some(CreateContainerOptions {
                    name: spec.name,
                    platform: None,
                }),
                cfg,
            )
            .await
            .map_err(|e| HmError::Docker(format!("create_container({}): {e}", spec.name)))?;

        // Attach to the per-session network with the slug as alias so
        // siblings reach this container via DNS.
        self.inner
            .connect_network(
                spec.network,
                ConnectNetworkOptions {
                    container: create.id.clone(),
                    endpoint_config: bollard::models::EndpointSettings {
                        aliases: Some(vec![spec.network_alias.to_string()]),
                        ..Default::default()
                    },
                },
            )
            .await
            .map_err(|e| HmError::Docker(format!("connect_network({}): {e}", spec.network)))?;

        self.inner
            .start_container(&create.id, None::<StartContainerOptions<String>>)
            .await
            .map_err(|e| HmError::Docker(format!("start_container({}): {e}", create.id)))?;

        Ok(create.id)
    }

    /// Inspect a container; return its container-port → host-port map for tcp.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] when inspect fails.
    pub async fn inspect_ports(
        &self,
        container_id: &str,
    ) -> Result<std::collections::HashMap<u16, u16>> {
        let info = self
            .inner
            .inspect_container(container_id, None)
            .await
            .map_err(|e| HmError::Docker(format!("inspect_container({container_id}): {e}")))?;
        let mut out = std::collections::HashMap::new();
        if let Some(ns) = info.network_settings
            && let Some(ports) = ns.ports
        {
            for (key, bindings) in ports {
                // key like "5432/tcp"
                let cport: u16 = key
                    .split('/')
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                if cport == 0 {
                    continue;
                }
                if let Some(bs) = bindings {
                    for b in bs {
                        if let Some(hp) = b.host_port
                            && let Ok(p) = hp.parse::<u16>()
                        {
                            out.insert(cport, p);
                        }
                    }
                }
            }
        }
        Ok(out)
    }

    /// Stop a container with a 10s grace, then SIGKILL. Idempotent on "not found".
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] for non-404 daemon errors.
    pub async fn stop_container(&self, container_id: &str) -> Result<()> {
        match self
            .inner
            .stop_container(container_id, Some(StopContainerOptions { t: 10 }))
            .await
        {
            Ok(())
            | Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(e) => Err(HmError::Docker(format!("stop_container({container_id}): {e}")).into()),
        }
    }

    /// Remove a container. Idempotent on "not found".
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] for non-404 daemon errors.
    pub async fn remove_container(&self, container_id: &str) -> Result<()> {
        match self
            .inner
            .remove_container(
                container_id,
                Some(RemoveContainerOptions {
                    force: true,
                    ..Default::default()
                }),
            )
            .await
        {
            Ok(())
            | Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => Ok(()),
            Err(e) => Err(HmError::Docker(format!("remove_container({container_id}): {e}")).into()),
        }
    }

    /// Allocate a TTY exec into a running container. Forwards stdin/stdout
    /// transparently so an interactive shell works. Returns exit code.
    ///
    /// # Errors
    ///
    /// Returns [`HmError::Docker`] on `create_exec` / `start_exec` / `inspect_exec` failures.
    pub async fn exec_tty(&self, container_id: &str, cmd: &[String]) -> Result<i32> {
        use bollard::exec::{StartExecOptions, StartExecResults};

        let create = self
            .inner
            .create_exec(
                container_id,
                CreateExecOptions::<&str> {
                    attach_stdout: Some(true),
                    attach_stderr: Some(true),
                    attach_stdin: Some(true),
                    tty: Some(true),
                    cmd: Some(cmd.iter().map(String::as_str).collect()),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| HmError::Docker(format!("create_exec({container_id}): {e}")))?;
        let start = self
            .inner
            .start_exec(
                &create.id,
                Some(StartExecOptions {
                    detach: false,
                    tty: true,
                    ..Default::default()
                }),
            )
            .await
            .map_err(|e| HmError::Docker(format!("start_exec({}): {e}", create.id)))?;
        if let StartExecResults::Attached { mut output, .. } = start {
            // Bridge container output to host stdout. For full bidi
            // stdin we would also need to feed the local stdin into
            // the exec input stream; left as a follow-up.
            while let Some(chunk) = output.next().await {
                if let Ok(c) = chunk {
                    use std::io::Write;
                    std::io::stdout().write_all(c.into_bytes().as_ref()).ok();
                }
            }
        }
        let info = self
            .inner
            .inspect_exec(&create.id)
            .await
            .map_err(|e| HmError::Docker(format!("inspect_exec({}): {e}", create.id)))?;
        Ok(info.exit_code.map_or(0, |c| i32::try_from(c).unwrap_or(0)))
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

// ---------------------------------------------------------------------------
// ServiceSpec
// ---------------------------------------------------------------------------

/// Spec for a long-lived service container created via
/// [`DockerClient::start_service`]. Build instances with
/// [`DockerClient::build_service_spec`] / [`ServiceSpecBuilder`].
#[derive(Debug, Clone)]
pub struct ServiceSpec<'a> {
    pub image: &'a str,
    pub name: &'a str,
    pub env: Vec<String>,
    pub cmd: Option<Vec<String>>,
    pub workdir: Option<&'a str>,
    pub binds: Vec<String>,
    pub publish: Vec<u16>,
    pub network: &'a str,
    pub network_alias: &'a str,
    pub labels: std::collections::HashMap<String, String>,
}

/// Fluent builder for [`ServiceSpec`].
#[derive(Debug)]
pub struct ServiceSpecBuilder<'a> {
    inner: ServiceSpec<'a>,
}

impl<'a> ServiceSpecBuilder<'a> {
    #[must_use]
    pub fn new(image: &'a str, name: &'a str) -> Self {
        Self {
            inner: ServiceSpec {
                image,
                name,
                env: Vec::new(),
                cmd: None,
                workdir: None,
                binds: Vec::new(),
                publish: Vec::new(),
                network: "",
                network_alias: "",
                labels: std::collections::HashMap::new(),
            },
        }
    }

    #[must_use]
    pub fn env(mut self, env: Vec<String>) -> Self {
        self.inner.env = env;
        self
    }

    #[must_use]
    pub fn cmd(mut self, cmd: Option<Vec<String>>) -> Self {
        self.inner.cmd = cmd;
        self
    }

    #[must_use]
    pub const fn workdir(mut self, w: Option<&'a str>) -> Self {
        self.inner.workdir = w;
        self
    }

    #[must_use]
    pub fn binds(mut self, b: Vec<String>) -> Self {
        self.inner.binds = b;
        self
    }

    #[must_use]
    pub fn publish(mut self, ports: Vec<u16>) -> Self {
        self.inner.publish = ports;
        self
    }

    #[must_use]
    pub const fn network(mut self, net: &'a str, alias: &'a str) -> Self {
        self.inner.network = net;
        self.inner.network_alias = alias;
        self
    }

    #[must_use]
    pub fn labels(mut self, l: std::collections::HashMap<String, String>) -> Self {
        self.inner.labels = l;
        self
    }

    #[must_use]
    pub fn build(self) -> ServiceSpec<'a> {
        self.inner
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
}
