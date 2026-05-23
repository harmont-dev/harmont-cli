//! Per-run state visible to host functions.
//!
//! The plan-1 host fns global `HostState` does not carry per-run
//! context — the orchestrator does. Host fns that consult per-run
//! state read it from this `OnceLock<Arc<OrchestratorState>>` that
//! the orchestrator installs at the start of each run.

// `clear()` is currently a no-op but is part of the lifecycle API
// the orchestrator calls at end-of-run; flipping it to `const fn`
// would break callers when we add an `OnceLock::take()`-like body
// in a future plan.
#![allow(clippy::missing_const_for_fn)]
// `expect`/`panic!` on the OnceLock install is the documented panic
// path for the "concurrent orchestrator runs" invariant; using `?`
// or returning a Result would force every caller into error handling
// for a programming-error case.
#![allow(clippy::expect_used)]
#![allow(clippy::panic)]

use std::sync::{Arc, OnceLock};

use uuid::Uuid;

use crate::orchestrator::docker_client::DockerClient;

use super::archive::ArchiveStore;
use tokio_util::sync::CancellationToken;
use super::events::EventBus;

/// Live state visible to every host fn while an orchestrator run is
/// active.
///
/// Hosted via the [`current()`] / [`install()`] / [`clear()`] trio so
/// host-fn implementations can read it without an extra
/// argument-passing channel.
#[derive(Debug)]
pub struct OrchestratorState {
    pub event_bus: Arc<EventBus>,
    pub archives: ArchiveStore,
    pub cancel: CancellationToken,
    pub docker: DockerClient,
    pub run_id: Uuid,
}

static CURRENT: OnceLock<Arc<OrchestratorState>> = OnceLock::new();

/// Install per-run state for the duration of an orchestrator run.
///
/// # Panics
///
/// Panics if state is installed twice — the host runs one
/// orchestrator at a time for plan 2.
pub fn install(state: Arc<OrchestratorState>) {
    assert!(
        CURRENT.set(state).is_ok(),
        "OrchestratorState already installed; concurrent orchestrator runs are not supported"
    );
}

/// Clear the installed state. Idempotent (no-op when nothing was
/// installed). Use at end-of-run.
pub fn clear() {
    // OnceLock has no take(); we leak the Arc on each run. The orchestrator
    // is invoked once per process lifetime today, so this is fine.
    // Long-running daemons that orchestrate would need a different shape.
}

/// Get a handle to the live state, if any.
#[must_use]
pub fn current() -> Option<Arc<OrchestratorState>> {
    CURRENT.get().cloned()
}
