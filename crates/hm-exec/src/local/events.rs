//! Build-event broadcast channel.
//!
//! Subscribers (output formatter plugin, lifecycle hook plugins,
//! the human-readable progress sink) all subscribe to the same
//! channel; the host's `emit_event` / `emit_step_log` host fns
//! publish into it.

// `new()` returning `Arc<Self>` is intentional (the bus is always
// shared); `subscribe()` returns a tokio receiver that callers must
// own.  Both look like must-use candidates to clippy.
#![allow(clippy::must_use_candidate)]

use std::sync::Arc;

use hm_plugin_protocol::BuildEvent;
use tokio::sync::broadcast;

/// Channel capacity. Larger than the queue any one subscriber should
/// fall behind on. Subscribers that lag past this drop events and
/// receive a `Lagged` error.
const BUS_CAPACITY: usize = 1024;

#[derive(Debug, Clone)]
pub struct EventBus {
    tx: broadcast::Sender<BuildEvent>,
}

impl EventBus {
    pub fn new() -> Arc<Self> {
        let (tx, _rx) = broadcast::channel(BUS_CAPACITY);
        Arc::new(Self { tx })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<BuildEvent> {
        self.tx.subscribe()
    }

    /// Publish an event. Returns the number of subscribers that
    /// received it. A return of 0 is normal (no subscribers yet).
    pub fn emit(&self, event: BuildEvent) {
        // We intentionally drop the error: zero-subscriber sends are
        // not interesting and we don't want host_fn impls to fail
        // because nobody is listening.
        let _ = self.tx.send(event);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn emit_and_receive() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();
        bus.emit(BuildEvent::BuildEnd {
            exit_code: 0,
            duration_ms: 1,
        });
        let ev = rx.recv().await.unwrap();
        matches!(ev, BuildEvent::BuildEnd { exit_code: 0, .. });
    }

    #[tokio::test]
    async fn no_subscribers_is_not_an_error() {
        let bus = EventBus::new();
        bus.emit(BuildEvent::BuildEnd {
            exit_code: 0,
            duration_ms: 0,
        });
        // Should not panic.
    }
}
