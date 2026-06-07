//! Boxlite (microVM) backend implementation.

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use boxlite::litebox::{BoxCommand, CopyOptions};
use boxlite::runtime::options::{BoxOptions, CloneOptions, RootfsSpec};
use boxlite::runtime::BoxliteRuntime;
use futures::StreamExt;

use crate::backend::{Vm, VmBackend};
use crate::types::{OutputSink, SnapshotId, VmConfig};

/// Boxlite-backed VM factory.
#[derive(Debug, Clone)]
pub struct BoxliteBackend {
    runtime: Arc<BoxliteRuntime>,
}

impl BoxliteBackend {
    /// Wrap an existing `BoxliteRuntime`.
    #[must_use]
    pub fn new(runtime: BoxliteRuntime) -> Self {
        Self {
            runtime: Arc::new(runtime),
        }
    }

    /// Create a backend using `BoxliteRuntime::with_defaults()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime cannot be initialised.
    pub fn with_defaults() -> Result<Self> {
        let runtime =
            BoxliteRuntime::with_defaults().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self::new(runtime))
    }
}

#[async_trait]
impl VmBackend for BoxliteBackend {
    async fn create(&self, image: &str, _config: &VmConfig) -> Result<Box<dyn Vm>> {
        let options = BoxOptions {
            rootfs: RootfsSpec::Image(image.to_owned()),
            auto_remove: false,
            detach: true,
            ..BoxOptions::default()
        };

        let litebox = self
            .runtime
            .create(options, None)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(Box::new(BoxliteVm { inner: litebox }))
    }

    async fn restore(&self, snapshot: &SnapshotId, _config: &VmConfig) -> Result<Box<dyn Vm>> {
        let parent = self
            .runtime
            .get(&snapshot.0)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?
            .ok_or_else(|| anyhow::anyhow!("snapshot {} not found", snapshot.0))?;

        let clone = parent
            .clone_box(CloneOptions::default(), None)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(Box::new(BoxliteVm { inner: clone }))
    }

    async fn snapshot_exists(&self, snapshot: &SnapshotId) -> Result<bool> {
        self.runtime
            .exists(&snapshot.0)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    async fn remove_snapshot(&self, snapshot: &SnapshotId) -> Result<()> {
        self.runtime
            .remove(&snapshot.0, true)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }
}

/// Handle to a running boxlite VM.
struct BoxliteVm {
    inner: boxlite::litebox::LiteBox,
}

#[async_trait]
impl Vm for BoxliteVm {
    async fn inject(&self, host_path: &Path, guest_path: &str) -> Result<()> {
        self.inner
            .copy_into(host_path, guest_path, CopyOptions::default())
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    async fn exec(
        &self,
        cmd: &str,
        env: &[(String, String)],
        working_dir: &str,
        sink: &dyn OutputSink,
    ) -> Result<i32> {
        let mut command = BoxCommand::new("sh")
            .args(["-c", cmd])
            .working_dir(working_dir);

        for (key, val) in env {
            command = command.env(key, val);
        }

        let mut execution = self
            .inner
            .exec(command)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut stdout = execution.stdout();
        let mut stderr = execution.stderr();

        // Drain both streams concurrently, forwarding to sink.
        match (stdout.as_mut(), stderr.as_mut()) {
            (Some(out), Some(err)) => {
                let mut out_done = false;
                let mut err_done = false;
                loop {
                    if out_done && err_done {
                        break;
                    }
                    tokio::select! {
                        item = out.next(), if !out_done => {
                            match item {
                                Some(data) => sink.on_stdout(&data),
                                None => out_done = true,
                            }
                        }
                        item = err.next(), if !err_done => {
                            match item {
                                Some(data) => sink.on_stderr(&data),
                                None => err_done = true,
                            }
                        }
                    }
                }
            }
            (Some(out), None) => {
                while let Some(data) = out.next().await {
                    sink.on_stdout(&data);
                }
            }
            (None, Some(err)) => {
                while let Some(data) = err.next().await {
                    sink.on_stderr(&data);
                }
            }
            (None, None) => {}
        }

        let result = execution
            .wait()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(result.exit_code)
    }

    async fn snapshot(&self, _label: &str) -> Result<SnapshotId> {
        // The stopped box IS the snapshot -- stop the VM and return its id.
        self.inner
            .stop()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(SnapshotId(self.inner.id().to_string()))
    }

    async fn destroy(&mut self) -> Result<()> {
        // Best-effort stop.
        let _ = self.inner.stop().await;
        Ok(())
    }
}
