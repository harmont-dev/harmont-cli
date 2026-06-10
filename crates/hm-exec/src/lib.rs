//! Pluggable CI execution backends for `hm run`.
//!
//! # Design
//!
//! The pluggable boundary is the **whole build**, not a single step.
//! [`ExecutionBackend::start`] accepts a [`RunRequest`] and returns a
//! [`BackendHandle`]. Calling [`BackendHandle::into_parts`] splits the handle
//! into:
//!
//! - An [`EventStream`] of [`hm_plugin_protocol::events::BuildEvent`]s — hand
//!   this to `hm-render` for terminal output.
//! - A [`Control`] struct with `cancel()` (Ctrl-C) and `wait()` (terminal
//!   outcome).
//!
//! # Backends
//!
//! - [`LocalBackend`] — runs the build in-process using a DAG scheduler that
//!   executes each step inside a lightweight VM via the `hm-vm` subsystem
//!   (a [`hm_vm::VmBackend`] + snapshot registry; Docker is one such backend).
//! - [`CloudBackend`] — submits the build to the Harmont cloud and watches it
//!   over the REST SDK, emitting the same `BuildEvent` stream.
//!
//! # Auth
//!
//! This crate never reads credentials from disk. The caller constructs a
//! `HarmontClient` and injects it; `hm` owns credential loading.
#![forbid(unsafe_code)]

mod error;
pub use error::{BackendError, Result};

mod request;
pub use request::{Plan, RunOptions, RunRequest, SourceMeta};

mod outcome;
pub use outcome::{BuildOutcome, BuildStatus, StepResultSummary, StepStatus};

mod capabilities;
pub use capabilities::Capabilities;

pub mod local;
pub use local::LocalBackend;

pub mod cloud;
pub use cloud::CloudBackend;

pub use hm_plugin_protocol::events::BuildRef;

use futures::StreamExt as _;
use hm_plugin_protocol::events::BuildEvent;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;
use tokio_util::sync::CancellationToken;

/// Type alias for the boxed event stream yielded by [`BackendHandle::into_parts`].
pub type EventStream = futures::stream::BoxStream<'static, BuildEvent>;

/// A pluggable execution backend. The boundary is the WHOLE build.
///
/// `start` spawns it and returns a [`BackendHandle`]. Per-step execution is a
/// private concern of the backend (see the local backend's internal
/// `StepRunner`).
#[async_trait::async_trait]
pub trait ExecutionBackend: Send + Sync {
    /// Stable id for diagnostics/telemetry ("local-docker", "cloud").
    fn name(&self) -> &str;
    /// What this backend can honor — consulted by the CLI before `start`.
    fn capabilities(&self) -> Capabilities;
    /// Begin running the whole build. Setup failures (auth, bad plan, no
    /// daemon) fail here; a *failed build* is `Ok(handle)` resolving to
    /// `BuildOutcome { status: Failed }`.
    async fn start(&self, req: RunRequest) -> Result<BackendHandle>;
}

/// A running build: an event stream to render + a control half (cancel/wait).
pub struct BackendHandle {
    events: EventStream,
    cancel: CancellationToken,
    outcome: JoinHandle<Result<BuildOutcome>>,
}

impl BackendHandle {
    /// Construct from a spawned run task that emits events into `rx` and
    /// resolves to an outcome.
    #[must_use]
    pub fn spawn(
        rx: mpsc::Receiver<BuildEvent>,
        cancel: CancellationToken,
        outcome: JoinHandle<Result<BuildOutcome>>,
    ) -> Self {
        Self {
            events: ReceiverStream::new(rx).boxed(),
            cancel,
            outcome,
        }
    }

    /// Split into the event stream (move into a renderer task) and control.
    #[must_use]
    pub fn into_parts(self) -> (EventStream, Control) {
        (
            self.events,
            Control {
                cancel: self.cancel,
                outcome: self.outcome,
            },
        )
    }
}

impl std::fmt::Debug for BackendHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendHandle").finish_non_exhaustive()
    }
}

/// The control half of a running build: cancel + await the outcome.
pub struct Control {
    cancel: CancellationToken,
    outcome: JoinHandle<Result<BuildOutcome>>,
}

impl Control {
    /// A clone of the cancellation token (hand to a Ctrl-C handler).
    #[must_use]
    pub fn cancel_token(&self) -> CancellationToken {
        self.cancel.clone()
    }
    /// Request cooperative cancellation (idempotent).
    pub fn cancel(&self) {
        self.cancel.cancel();
    }
    /// Await the terminal outcome.
    ///
    /// # Errors
    /// Returns [`BackendError::Other`] if the spawned task panicked, or any
    /// [`BackendError`] the backend task itself returned.
    pub async fn wait(self) -> Result<BuildOutcome> {
        match self.outcome.await {
            Ok(res) => res,
            Err(join_err) => Err(BackendError::Other(Box::new(join_err))),
        }
    }
}

impl std::fmt::Debug for Control {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Control").finish_non_exhaustive()
    }
}
