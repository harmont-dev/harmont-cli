//! Bridges OS signals to the orchestrator's `CancellationToken`.
//!
//! Today's hm process: a single tokio runtime serving one CLI command.
//! Ctrl-C should: (1) flip the token so runners drain quickly; (2)
//! exit with code 130 (sigint).

// Pedantic-bucket nags accepted at module scope:
// - `exit`: force-exit on second Ctrl-C is the documented UX, matching
//   the legacy executor. The user has explicitly asked us to die.
#![allow(clippy::exit)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio_util::sync::CancellationToken;

/// Spawn a tokio task that listens for SIGINT (Ctrl-C) and flips
/// the token. Returns a handle; aborting the handle is sufficient
/// cleanup since the runtime tears down on process exit.
///
/// On second Ctrl-C, the task force-exits with code 130 — same UX
/// as the legacy executor.
#[must_use = "drop the JoinHandle to leak the listener; bind to a `_` to tie its lifetime to the caller scope"]
pub(crate) fn install_ctrlc(token: CancellationToken) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let armed = Arc::new(AtomicBool::new(false));
        loop {
            match tokio::signal::ctrl_c().await {
                Ok(()) => {
                    if armed.swap(true, Ordering::SeqCst) {
                        tracing::warn!("\nforce-exit on second Ctrl-C");
                        std::process::exit(130);
                    }
                    tracing::info!("\ncancelling… (Ctrl-C again to force)");
                    token.cancel();
                }
                Err(_) => return,
            }
        }
    })
}
