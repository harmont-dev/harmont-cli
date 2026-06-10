//! Human-readable [`OutputRenderer`] — replaces the former
//! `hm-plugin-output-human` WASM plugin with a plain struct that
//! writes formatted lines to any [`std::io::Write`] target.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;

use hm_plugin_protocol::BuildEvent;
use owo_colors::{AnsiColors, OwoColorize};
use uuid::Uuid;

use crate::OutputRenderer;

/// Renders [`BuildEvent`]s as human-readable log lines.
///
/// Generic over the writer so tests can capture output into a
/// `Vec<u8>` while production code writes to `stderr`.
#[derive(Debug)]
pub struct HumanRenderer<W> {
    out: W,
    step_keys: HashMap<Uuid, String>,
    color: bool,
}

impl<W> HumanRenderer<W> {
    /// Create a new renderer writing to `out`.
    #[must_use]
    pub fn new(out: W, color: bool) -> Self {
        Self {
            out,
            step_keys: HashMap::new(),
            color,
        }
    }
}

fn key_color(key: &str) -> AnsiColors {
    const PALETTE: [AnsiColors; 6] = [
        AnsiColors::Cyan,
        AnsiColors::Magenta,
        AnsiColors::Yellow,
        AnsiColors::Green,
        AnsiColors::Blue,
        AnsiColors::BrightRed,
    ];
    let mut h: u32 = 0;
    for b in key.bytes() {
        h = h.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    PALETTE[(h as usize) % PALETTE.len()]
}

fn fmt_key(key: &str, color: bool) -> String {
    if color {
        format!("[{}]", key.color(key_color(key)))
    } else {
        format!("[{key}]")
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
                let prefix = fmt_key(self.step_key(step_id), self.color);
                image
                    .as_ref()
                    .map_or_else(
                        || format!("{prefix} start (runner={runner})\n"),
                        |img| format!("{prefix} start (runner={runner} image={img})\n"),
                    )
                    .into_bytes()
            }

            BuildEvent::StepLog { step_id, line, .. } => {
                let prefix = fmt_key(self.step_key(step_id), self.color);
                format!("{prefix} {line}\n").into_bytes()
            }

            BuildEvent::StepCacheHit { step_id, tag, .. } => {
                let prefix = fmt_key(self.step_key(step_id), self.color);
                format!("{prefix} cache hit ({tag})\n").into_bytes()
            }

            BuildEvent::StepEnd {
                step_id,
                exit_code,
                duration_ms,
                ..
            } => {
                let prefix = fmt_key(self.step_key(step_id), self.color);
                format!("{prefix} end exit={exit_code} duration={duration_ms}ms\n").into_bytes()
            }

            BuildEvent::BuildEnd {
                exit_code,
                duration_ms,
            } => format!("build: end exit={exit_code} duration={duration_ms}ms\n").into_bytes(),

            BuildEvent::BuildAccepted {
                build,
                watch_url: Some(url),
            } => {
                let n = build.number.map(|n| format!("#{n} ")).unwrap_or_default();
                format!("build {n}\u{2192} {url}\n").into_bytes()
            }

            BuildEvent::ChainFailed {
                chain_idx,
                failed_step_key,
                exit_code,
                message,
                ..
            } => {
                let styled_key = if self.color {
                    format!("{}", failed_step_key.color(key_color(failed_step_key)))
                } else {
                    failed_step_key.clone()
                };
                format!(
                    "chain {chain_idx}: FAILED at step '{styled_key}' (exit={exit_code}): {message}\n"
                )
                .into_bytes()
            }

            _ => return, // unknown future event: no visible output
        };

        let _ = self.out.write_all(&bytes);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use hm_plugin_protocol::{PlanSummary, StdStream};

    /// Helper: create a renderer backed by an in-memory buffer (no color).
    fn renderer() -> HumanRenderer<Vec<u8>> {
        HumanRenderer::new(Vec::new(), false)
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

    #[test]
    fn colored_output_wraps_key_in_ansi() {
        let mut r = HumanRenderer::new(Vec::new(), true);
        let step_id = Uuid::new_v4();

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
            line: "hello".into(),
            ts: chrono::Utc::now(),
        });

        let s = output(&r);
        assert!(s.contains("\x1b["), "expected ANSI codes: {s}");
        assert!(s.contains("hello"), "expected log line: {s}");
    }
}
