//! Progress-bar [`OutputRenderer`] — bridges [`BuildEvent`]s into
//! `tracing` spans that `tracing-indicatif` renders as live progress
//! bars.
//!
//! Each pipeline step gets its own child span (and therefore its own
//! progress bar). Logs are buffered silently and only replayed to the
//! writer on failure, keeping the TUI clean during normal runs.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;

use hm_plugin_protocol::BuildEvent;
use indicatif::ProgressStyle;
use tracing::{info_span, Span};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use uuid::Uuid;

use crate::runner::OutputRenderer;

/// Progress-bar renderer.
///
/// Generic over `W: Write` so tests can capture text output into a
/// `Vec<u8>` while production code writes to `std::io::Stderr`.
pub struct ProgressRenderer<W> {
    out: W,
    root_span: Option<Span>,
    step_spans: HashMap<Uuid, Span>,
    step_keys: HashMap<Uuid, String>,
    log_buffer: HashMap<Uuid, Vec<String>>,
    failed_steps: Vec<(Uuid, i32)>,
}

impl<W> fmt::Debug for ProgressRenderer<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressRenderer")
            .field("steps_tracked", &self.step_spans.len())
            .finish()
    }
}

impl<W> ProgressRenderer<W> {
    /// Create a new renderer writing failure reports to `out`.
    #[must_use]
    pub fn new(out: W) -> Self {
        Self {
            out,
            root_span: None,
            step_spans: HashMap::new(),
            step_keys: HashMap::new(),
            log_buffer: HashMap::new(),
            failed_steps: Vec::new(),
        }
    }
}

impl<W: Write> ProgressRenderer<W> {
    /// Print buffered logs for every failed step.
    fn print_failure_report(&mut self) {
        for (step_id, exit_code) in &self.failed_steps {
            let key = self.step_keys.get(step_id).map_or("?", String::as_str);
            let _ = writeln!(self.out, "\n--- {key} failed (exit {exit_code}) ---");
            if let Some(lines) = self.log_buffer.get(step_id) {
                for line in lines {
                    let _ = writeln!(self.out, "{line}");
                }
            }
        }
    }
}

impl<W> OutputRenderer for ProgressRenderer<W>
where
    W: Write + Send + fmt::Debug,
{
    fn on_event(&mut self, event: &BuildEvent) {
        match event {
            BuildEvent::BuildStart { plan, .. } => {
                let root = info_span!("pipeline");

                root.pb_set_style(
                    &ProgressStyle::with_template(
                        "{spinner} {span_name}  {wide_bar} {pos}/{len} steps  ({elapsed})",
                    )
                    .unwrap_or_else(|_| ProgressStyle::default_bar()),
                );
                root.pb_set_length(plan.step_count as u64);
                root.pb_start();

                self.root_span = Some(root);
            }

            BuildEvent::StepQueued {
                step_id, key, ..
            } => {
                self.step_keys.insert(*step_id, key.clone());

                let span = if let Some(root) = &self.root_span {
                    info_span!(parent: root, "step", name = %key)
                } else {
                    info_span!("step", name = %key)
                };

                span.pb_set_style(
                    &ProgressStyle::with_template(
                        "{span_child_prefix}{spinner} {span_fields}  {wide_msg}  ({elapsed})",
                    )
                    .unwrap_or_else(|_| ProgressStyle::default_spinner()),
                );
                span.pb_set_message("queued");
                span.pb_start();

                self.step_spans.insert(*step_id, span);
            }

            BuildEvent::StepStart {
                step_id,
                runner,
                image,
            } => {
                if let Some(span) = self.step_spans.get(step_id) {
                    let msg = image.as_ref().map_or_else(
                        || format!("running ({runner})"),
                        |img| format!("running ({runner} {img})"),
                    );
                    span.pb_set_message(&msg);
                }
            }

            BuildEvent::StepLog {
                step_id, line, ..
            } => {
                self.log_buffer
                    .entry(*step_id)
                    .or_default()
                    .push(line.clone());
            }

            BuildEvent::StepCacheHit { step_id, .. } => {
                if let Some(span) = self.step_spans.get(step_id) {
                    span.pb_set_message("cached");
                }
            }

            BuildEvent::StepEnd {
                step_id,
                exit_code,
                ..
            } => {
                if *exit_code != 0 {
                    self.failed_steps.push((*step_id, *exit_code));
                }

                // Dropping the span removes the progress bar.
                self.step_spans.remove(step_id);

                if let Some(root) = &self.root_span {
                    root.pb_inc(1);
                }
            }

            BuildEvent::ChainFailed { .. } => {}

            BuildEvent::BuildEnd { exit_code, .. } => {
                // Clear all remaining spans (removes all progress bars).
                self.step_spans.clear();
                self.root_span.take();

                if *exit_code != 0 {
                    self.print_failure_report();
                }
            }
        }
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
    fn renderer() -> ProgressRenderer<Vec<u8>> {
        ProgressRenderer::new(Vec::new())
    }

    /// Helper: drain the buffer as a UTF-8 string.
    fn output(r: &ProgressRenderer<Vec<u8>>) -> String {
        String::from_utf8(r.out.clone()).unwrap()
    }

    #[test]
    fn buffers_logs_silently() {
        let mut r = renderer();
        let step_id = Uuid::new_v4();

        r.on_event(&BuildEvent::StepQueued {
            step_id,
            key: "compile".into(),
            chain_idx: 0,
        });

        r.on_event(&BuildEvent::StepLog {
            step_id,
            stream: StdStream::Stdout,
            line: "compiling main.rs".into(),
            ts: chrono::Utc::now(),
        });

        // No text output — progress bars handle display, logs are buffered.
        assert!(output(&r).is_empty(), "expected no text output");

        // But the log line IS buffered internally.
        let buf = r.log_buffer.get(&step_id).expect("log_buffer entry");
        assert_eq!(buf.len(), 1);
        assert_eq!(buf[0], "compiling main.rs");
    }

    #[test]
    fn replays_logs_on_failure() {
        let mut r = renderer();
        let step_id = Uuid::new_v4();

        r.on_event(&BuildEvent::BuildStart {
            run_id: Uuid::nil(),
            plan: PlanSummary {
                step_count: 1,
                chain_count: 1,
                default_runner: "docker".into(),
            },
            started_at: chrono::Utc::now(),
        });

        r.on_event(&BuildEvent::StepQueued {
            step_id,
            key: "test".into(),
            chain_idx: 0,
        });

        r.on_event(&BuildEvent::StepLog {
            step_id,
            stream: StdStream::Stderr,
            line: "assertion failed at line 42".into(),
            ts: chrono::Utc::now(),
        });

        r.on_event(&BuildEvent::StepEnd {
            step_id,
            exit_code: 1,
            duration_ms: 500,
            snapshot: None,
        });

        r.on_event(&BuildEvent::BuildEnd {
            exit_code: 1,
            duration_ms: 600,
        });

        let s = output(&r);
        assert!(s.contains("test"), "expected step key in output: {s}");
        assert!(s.contains("exit 1"), "expected exit code in output: {s}");
        assert!(
            s.contains("assertion failed at line 42"),
            "expected log line in output: {s}"
        );
    }

    #[test]
    fn no_output_on_success() {
        let mut r = renderer();
        let step_id = Uuid::new_v4();

        r.on_event(&BuildEvent::BuildStart {
            run_id: Uuid::nil(),
            plan: PlanSummary {
                step_count: 1,
                chain_count: 1,
                default_runner: "docker".into(),
            },
            started_at: chrono::Utc::now(),
        });

        r.on_event(&BuildEvent::StepQueued {
            step_id,
            key: "build".into(),
            chain_idx: 0,
        });

        r.on_event(&BuildEvent::StepLog {
            step_id,
            stream: StdStream::Stdout,
            line: "all good".into(),
            ts: chrono::Utc::now(),
        });

        r.on_event(&BuildEvent::StepEnd {
            step_id,
            exit_code: 0,
            duration_ms: 200,
            snapshot: None,
        });

        r.on_event(&BuildEvent::BuildEnd {
            exit_code: 0,
            duration_ms: 250,
        });

        // Success path: no text output (progress bars handled display).
        assert!(
            output(&r).is_empty(),
            "expected no text output on success: {:?}",
            output(&r)
        );
    }
}
