//! Human-readable [`OutputRenderer`] — replaces the former
//! `hm-plugin-output-human` WASM plugin with a plain struct that
//! writes formatted lines to any [`std::io::Write`] target.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;

use hm_plugin_protocol::BuildEvent;
use uuid::Uuid;

use crate::runner::OutputRenderer;

/// Renders [`BuildEvent`]s as human-readable log lines.
///
/// Generic over the writer so tests can capture output into a
/// `Vec<u8>` while production code writes to `stderr`.
#[derive(Debug)]
pub struct HumanRenderer<W> {
    out: W,
    step_keys: HashMap<Uuid, String>,
}

impl<W> HumanRenderer<W> {
    /// Create a new renderer writing to `out`.
    #[must_use]
    pub fn new(out: W) -> Self {
        Self {
            out,
            step_keys: HashMap::new(),
        }
    }
}

impl<W> HumanRenderer<W>
where
    W: Write,
{
    /// Look up the human-readable key for a step, falling back to `"?"`.
    fn step_key(&self, id: &Uuid) -> &str {
        self.step_keys.get(id).map_or("?", String::as_str)
    }
}

impl<W> OutputRenderer for HumanRenderer<W>
where
    W: Write + Send + fmt::Debug,
{
    fn on_event(&mut self, event: &BuildEvent) {
        let bytes: Vec<u8> = match event {
            BuildEvent::BuildStart { plan, .. } => format!(
                "build: {} steps in {} chain(s)\n",
                plan.step_count, plan.chain_count,
            )
            .into_bytes(),

            BuildEvent::StepQueued { step_id, key, .. } => {
                self.step_keys.insert(*step_id, key.clone());
                return; // no visible output
            }

            BuildEvent::StepStart {
                step_id,
                runner,
                image,
            } => {
                let key = self.step_key(step_id);
                image.as_ref().map_or_else(
                    || format!("[{key}] start (runner={runner})\n"),
                    |img| format!("[{key}] start (runner={runner} image={img})\n"),
                )
                .into_bytes()
            }

            BuildEvent::StepLog {
                step_id, line, ..
            } => {
                let key = self.step_key(step_id);
                format!("[{key}] {line}\n").into_bytes()
            }

            BuildEvent::StepCacheHit {
                step_id, tag, ..
            } => {
                let key = self.step_key(step_id);
                format!("[{key}] cache hit ({tag})\n").into_bytes()
            }

            BuildEvent::StepEnd {
                step_id,
                exit_code,
                duration_ms,
                ..
            } => {
                let key = self.step_key(step_id);
                format!("[{key}] end exit={exit_code} duration={duration_ms}ms\n").into_bytes()
            }

            BuildEvent::BuildEnd {
                exit_code,
                duration_ms,
            } => {
                format!("build: end exit={exit_code} duration={duration_ms}ms\n").into_bytes()
            }

            BuildEvent::ChainFailed {
                chain_idx,
                failed_step_key,
                exit_code,
                message,
                ..
            } => format!(
                "chain {chain_idx}: FAILED at step '{failed_step_key}' (exit={exit_code}): {message}\n"
            )
            .into_bytes(),
        };

        let _ = self.out.write_all(&bytes);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use hm_plugin_protocol::{PlanSummary, StdStream};

    /// Helper: create a renderer backed by an in-memory buffer.
    fn renderer() -> HumanRenderer<Vec<u8>> {
        HumanRenderer::new(Vec::new())
    }

    /// Helper: drain the buffer as a UTF-8 string.
    fn output(r: &HumanRenderer<Vec<u8>>) -> String {
        String::from_utf8(r.out.clone()).unwrap()
    }

    #[test]
    fn build_start_renders_counts() {
        let mut r = renderer();
        r.on_event(&BuildEvent::BuildStart {
            run_id: Uuid::nil(),
            plan: PlanSummary {
                step_count: 5,
                chain_count: 3,
                default_runner: "docker".into(),
            },
            started_at: chrono::Utc::now(),
        });

        let s = output(&r);
        assert!(s.contains("5 steps"), "expected step count: {s}");
        assert!(s.contains("3 chain(s)"), "expected chain count: {s}");
    }

    #[test]
    fn step_log_with_key() {
        let mut r = renderer();
        let step_id = Uuid::new_v4();

        // Queue the step so the key is recorded.
        r.on_event(&BuildEvent::StepQueued {
            step_id,
            key: "build".into(),
            chain_idx: 0,
            parent_key: None,
            display_name: "build".into(),
        });

        r.on_event(&BuildEvent::StepLog {
            step_id,
            stream: StdStream::Stdout,
            line: "compiling...".into(),
            ts: chrono::Utc::now(),
        });

        let s = output(&r);
        assert_eq!(s, "[build] compiling...\n");
    }

    #[test]
    fn step_log_unknown_key() {
        let mut r = renderer();

        // Emit a log without a prior StepQueued.
        r.on_event(&BuildEvent::StepLog {
            step_id: Uuid::new_v4(),
            stream: StdStream::Stdout,
            line: "orphan line".into(),
            ts: chrono::Utc::now(),
        });

        let s = output(&r);
        assert!(s.starts_with("[?]"), "expected [?] prefix: {s}");
    }
}
