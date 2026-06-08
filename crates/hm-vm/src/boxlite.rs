//! Boxlite (microVM) backend implementation.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use async_trait::async_trait;
use boxlite::litebox::{BoxCommand, CopyOptions};
use boxlite::runtime::BoxliteRuntime;
use boxlite::runtime::options::{BoxOptions, CloneOptions, RootfsSpec};
use futures::StreamExt;
use tokio::sync::Semaphore;
use tracing::instrument;

use crate::backend::{Vm, VmBackend};
use crate::types::{OutputSink, SnapshotId, VmConfig};

/// Boxlite-backed VM factory.
#[derive(derive_more::Debug, Clone)]
pub struct BoxliteBackend {
    runtime: Arc<BoxliteRuntime>,
    /// Serialises VM starts so only one libkrun boot proceeds at a time.
    #[debug(skip)]
    start_gate: Arc<Semaphore>,
}

impl BoxliteBackend {
    /// Wrap an existing `BoxliteRuntime`.
    #[must_use]
    pub fn new(runtime: BoxliteRuntime) -> Self {
        Self {
            runtime: Arc::new(runtime),
            start_gate: Arc::new(Semaphore::new(1)),
        }
    }

    /// Create a backend using `BoxliteRuntime::with_defaults()`.
    ///
    /// # Errors
    ///
    /// Returns an error if the runtime cannot be initialised.
    pub fn with_defaults() -> Result<Self> {
        let runtime = BoxliteRuntime::with_defaults().map_err(|e| anyhow::anyhow!("{e}"))?;
        Ok(Self::new(runtime))
    }
}

#[async_trait]
impl VmBackend for BoxliteBackend {
    #[instrument(skip(self, config))]
    async fn create(&self, image: &str, config: &VmConfig) -> Result<Box<dyn Vm>> {
        let options = BoxOptions {
            rootfs: RootfsSpec::Image(image.to_owned()),
            cpus: config.cpus.and_then(|c| u8::try_from(c).ok()),
            memory_mib: config.memory_mib.and_then(|m| u32::try_from(m).ok()),
            disk_size_gb: config.disk_size_gb,
            auto_remove: false,
            detach: true,
            ..BoxOptions::default()
        };

        let litebox = self
            .runtime
            .create(options, None)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(Box::new(BoxliteVm {
            inner: litebox,
            stopped: false,
            start_gate: Arc::clone(&self.start_gate),
            started: AtomicBool::new(false),
        }))
    }

    #[instrument(skip(self, _config))]
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

        Ok(Box::new(BoxliteVm {
            inner: clone,
            stopped: false,
            start_gate: Arc::clone(&self.start_gate),
            started: AtomicBool::new(false),
        }))
    }

    #[instrument(skip(self))]
    async fn snapshot_exists(&self, snapshot: &SnapshotId) -> Result<bool> {
        self.runtime
            .exists(&snapshot.0)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    #[instrument(skip(self))]
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
    stopped: bool,
    /// Shared gate that serialises VM starts across sibling VMs.
    start_gate: Arc<Semaphore>,
    /// Whether this VM has been started at least once.
    started: AtomicBool,
}

impl BoxliteVm {
    /// Serialize VM initialization across all VMs in this backend.
    ///
    /// Acquires the shared start gate, calls `LiteBox::start()`, then
    /// releases. Already-started VMs skip the gate entirely.
    #[instrument(skip(self), fields(box_id = %self.inner.id()))]
    async fn ensure_started(&self) -> Result<()> {
        if self.started.load(Ordering::Acquire) {
            return Ok(());
        }

        let _permit = self
            .start_gate
            .acquire()
            .await
            .map_err(|_| anyhow::anyhow!("start gate closed"))?;

        // Double-check: another caller may have started us while we waited.
        if self.started.load(Ordering::Acquire) {
            return Ok(());
        }

        self.inner
            .start()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        self.started.store(true, Ordering::Release);
        Ok(())
    }
}

#[async_trait]
impl Vm for BoxliteVm {
    #[instrument(skip(self), fields(host = %host_path.display()))]
    async fn inject(&self, host_path: &Path, guest_path: &str) -> Result<()> {
        self.ensure_started().await?;
        let opts = CopyOptions::default().include_parent(false);
        self.inner
            .copy_into(host_path, guest_path, opts)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))
    }

    #[instrument(skip(self, env, sink))]
    async fn exec(
        &self,
        cmd: &str,
        env: &[(String, String)],
        working_dir: &str,
        sink: &dyn OutputSink,
    ) -> Result<i32> {
        self.ensure_started().await?;
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

        let result = execution.wait().await.map_err(|e| anyhow::anyhow!("{e}"))?;

        Ok(result.exit_code)
    }

    #[instrument(skip(self))]
    async fn snapshot(&mut self, _label: &str) -> Result<SnapshotId> {
        self.inner
            .stop()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        self.stopped = true;
        Ok(SnapshotId(self.inner.id().to_string()))
    }

    #[instrument(skip(self))]
    async fn destroy(&mut self) -> Result<()> {
        if !self.stopped {
            let _ = self.inner.stop().await;
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn start_gate_serializes_access() {
        let gate = Arc::new(tokio::sync::Semaphore::new(1));
        let concurrent_count = Arc::new(AtomicUsize::new(0));
        let max_concurrent = Arc::new(AtomicUsize::new(0));

        let mut handles = vec![];
        for _ in 0..5 {
            let gate = Arc::clone(&gate);
            let concurrent = Arc::clone(&concurrent_count);
            let max = Arc::clone(&max_concurrent);

            handles.push(tokio::spawn(async move {
                let _permit = gate.acquire().await.unwrap();
                let prev = concurrent.fetch_add(1, Ordering::SeqCst);
                max.fetch_max(prev + 1, Ordering::SeqCst);
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                concurrent.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        assert_eq!(max_concurrent.load(Ordering::SeqCst), 1);
    }
}
