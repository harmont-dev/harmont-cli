//! Build-event subscriber that passes events to an [`OutputRenderer`].
//!
//! Replaces the plan-2 plugin-based output subscriber with a simple
//! loop that calls [`OutputRenderer::on_event`] for each bus event.

// Pedantic-bucket nags accepted at module scope:
// - `needless_pass_by_value` on `bus`: the owned `Arc<EventBus>` makes
//   the bus->subscriber handoff explicit at the call site.
#![allow(clippy::needless_pass_by_value)]

use std::sync::Arc;

use tokio::sync::broadcast::error::RecvError;

use super::events::EventBus;
use crate::runner::OutputRenderer;

/// Spawn the subscriber task. Returns a join handle the orchestrator
/// awaits at shutdown so the `BuildEnd` event is fully drained.
#[must_use]
pub fn spawn(
    bus: Arc<EventBus>,
    mut renderer: Box<dyn OutputRenderer>,
) -> tokio::task::JoinHandle<()> {
    let mut rx = bus.subscribe();
    tokio::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(event) => {
                    let is_end = event.is_build_end();
                    renderer.on_event(&event);
                    if is_end {
                        return;
                    }
                }
                Err(RecvError::Closed) => return,
                Err(RecvError::Lagged(n)) => {
                    tracing::warn!("output: dropped {n} events");
                }
            }
        }
    })
}
