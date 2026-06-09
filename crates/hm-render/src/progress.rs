//! Progress-bar [`OutputRenderer`] — bridges [`BuildEvent`]s into
//! `tracing` spans that `tracing-indicatif` renders as live progress
//! bars.
//!
//! Each pipeline step gets its own child span (and therefore its own
//! progress bar). Completed steps stay visible with a ✓/✗ indicator;
//! only actively running steps show a spinner. Logs are buffered
//! silently and only replayed to the writer on failure.

use std::collections::HashMap;
use std::fmt;
use std::io::Write;

use hm_plugin_protocol::BuildEvent;
use indicatif::ProgressStyle;
use owo_colors::{OwoColorize, Style};
use tracing::{Span, info_span};
use tracing_indicatif::span_ext::IndicatifSpanExt;
use uuid::Uuid;

use crate::OutputRenderer;

fn styled(text: &str, style: Style, color: bool) -> String {
    if color {
        format!("{}", text.style(style))
    } else {
        text.to_string()
    }
}

#[allow(clippy::literal_string_with_formatting_args)]
fn active_style(color: bool) -> ProgressStyle {
    let tpl = if color {
        "{span_child_prefix}{spinner:.cyan} {wide_msg}  ({elapsed})"
    } else {
        "{span_child_prefix}{spinner} {wide_msg}  ({elapsed})"
    };
    ProgressStyle::with_template(tpl).unwrap_or_else(|_| ProgressStyle::default_spinner())
}

#[allow(clippy::literal_string_with_formatting_args)]
fn completed_style(color: bool) -> ProgressStyle {
    let check = if color {
        format!("{}", "✓".green())
    } else {
        "✓".to_string()
    };
    let tpl = format!("{{span_child_prefix}}{check} {{wide_msg}}");
    ProgressStyle::with_template(&tpl).unwrap_or_else(|_| ProgressStyle::default_spinner())
}

#[allow(clippy::literal_string_with_formatting_args)]
fn failed_style(color: bool) -> ProgressStyle {
    let cross = if color {
        format!("{}", "✗".red())
    } else {
        "✗".to_string()
    };
    let tpl = format!("{{span_child_prefix}}{cross} {{wide_msg}}");
    ProgressStyle::with_template(&tpl).unwrap_or_else(|_| ProgressStyle::default_spinner())
}

fn format_duration(ms: u64) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else if ms < 60_000 {
        let secs = ms / 1000;
        let tenths = (ms % 1000) / 100;
        format!("{secs}.{tenths}s")
    } else {
        let mins = ms / 60_000;
        let secs = (ms % 60_000) / 1000;
        format!("{mins}m{secs}s")
    }
}

/// Progress-bar renderer.
///
/// Generic over `W: Write` so tests can capture text output into a
/// `Vec<u8>` while production code writes to `std::io::Stderr`.
#[derive(Debug)]
pub(crate) enum StepOutcome {
    Succeeded { duration_ms: u64 },
    Failed { duration_ms: u64, exit_code: i32 },
    Cancelled { duration_ms: u64 },
    Cached,
}

pub struct ProgressRenderer<W> {
    out: W,
    pub(crate) color: bool,
    root_span: Option<Span>,
    step_spans: HashMap<Uuid, Span>,
    step_keys: HashMap<Uuid, String>,
    step_names: HashMap<Uuid, String>,
    log_buffer: HashMap<Uuid, Vec<String>>,
    failed_steps: Vec<(Uuid, i32)>,
    step_order: Vec<Uuid>,
    pub(crate) step_outcomes: HashMap<Uuid, StepOutcome>,
}

impl<W> fmt::Debug for ProgressRenderer<W> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProgressRenderer")
            .field("steps_tracked", &self.step_spans.len())
            .finish_non_exhaustive()
    }
}

impl<W> ProgressRenderer<W> {
    #[must_use]
    pub fn new(out: W, color: bool) -> Self {
        Self {
            out,
            color,
            root_span: None,
            step_spans: HashMap::new(),
            step_keys: HashMap::new(),
            step_names: HashMap::new(),
            log_buffer: HashMap::new(),
            failed_steps: Vec::new(),
            step_order: Vec::new(),
            step_outcomes: HashMap::new(),
        }
    }
}

impl<W: Write> ProgressRenderer<W> {
    fn print_failure_report(&mut self) {
        for (step_id, exit_code) in &self.failed_steps {
            let name = self.step_names.get(step_id).map_or("?", String::as_str);
            let header = format!("--- {name} failed (exit {exit_code}) ---");
            let _ = writeln!(
                self.out,
                "\n{}",
                styled(&header, Style::new().red(), self.color)
            );
            if let Some(lines) = self.log_buffer.get(step_id) {
                for line in lines {
                    let _ = writeln!(self.out, "{line}");
                }
            }
        }
    }

    fn print_step_summary(&mut self) {
        let max_name_len = self
            .step_order
            .iter()
            .filter_map(|id| self.step_names.get(id))
            .map(String::len)
            .max()
            .unwrap_or(0);

        let _ = writeln!(self.out);
        for step_id in &self.step_order {
            let name = self.step_names.get(step_id).map_or("?", String::as_str);
            let (indicator, timing) = match self.step_outcomes.get(step_id) {
                Some(StepOutcome::Succeeded { duration_ms }) => (
                    styled("✓", Style::new().green(), self.color),
                    styled(
                        &format_duration(*duration_ms),
                        Style::new().dimmed(),
                        self.color,
                    ),
                ),
                Some(StepOutcome::Failed {
                    duration_ms,
                    exit_code,
                }) => (
                    styled("✗", Style::new().red(), self.color),
                    styled(
                        &format!("{}  exit {exit_code}", format_duration(*duration_ms)),
                        Style::new().red(),
                        self.color,
                    ),
                ),
                Some(StepOutcome::Cancelled { duration_ms }) => (
                    styled("-", Style::new().dimmed(), self.color),
                    styled(
                        &format!("{}  cancelled", format_duration(*duration_ms)),
                        Style::new().dimmed(),
                        self.color,
                    ),
                ),
                Some(StepOutcome::Cached) => (
                    styled("✓", Style::new().green(), self.color),
                    styled("cached", Style::new().dimmed(), self.color),
                ),
                None => (
                    styled("-", Style::new().dimmed(), self.color),
                    styled("—", Style::new().dimmed(), self.color),
                ),
            };
            let _ = writeln!(self.out, "  {indicator} {name:<max_name_len$}  {timing}");
        }
    }
}

impl<W> OutputRenderer for ProgressRenderer<W>
where
    W: Write + Send + fmt::Debug,
{
    #[allow(clippy::too_many_lines, clippy::literal_string_with_formatting_args)]
    fn on_event(&mut self, event: &BuildEvent) {
        match event {
            BuildEvent::BuildStart { plan, .. } => {
                let root = info_span!("pipeline");

                let tpl = if self.color {
                    "{spinner:.green} {span_name}  {wide_bar:.green/white} {pos}/{len} steps  ({elapsed})"
                } else {
                    "{spinner} {span_name}  {wide_bar} {pos}/{len} steps  ({elapsed})"
                };
                root.pb_set_style(
                    &ProgressStyle::with_template(tpl)
                        .unwrap_or_else(|_| ProgressStyle::default_bar()),
                );
                root.pb_set_length(plan.step_count as u64);
                root.pb_start();

                self.root_span = Some(root);
            }

            BuildEvent::StepQueued {
                step_id,
                key,
                parent_key,
                display_name,
                ..
            } => {
                self.step_keys.insert(*step_id, key.clone());
                self.step_names.insert(*step_id, display_name.clone());
                self.step_order.push(*step_id);

                let parent_span = parent_key
                    .as_ref()
                    .and_then(|pk| {
                        self.step_keys
                            .iter()
                            .find(|(_, k)| *k == pk)
                            .and_then(|(id, _)| self.step_spans.get(id))
                    })
                    .or(self.root_span.as_ref());

                let span = parent_span
                    .map_or_else(|| info_span!("step"), |p| info_span!(parent: p, "step"));

                span.pb_set_style(&active_style(self.color));
                span.pb_set_message(display_name);
                span.pb_start();

                self.step_spans.insert(*step_id, span);
            }

            BuildEvent::StepStart { step_id, .. } => {
                if let Some(span) = self.step_spans.get(step_id) {
                    let name = self.step_names.get(step_id).map_or("?", String::as_str);
                    span.pb_set_message(name);
                }
            }

            BuildEvent::StepLog { step_id, line, .. } => {
                self.log_buffer
                    .entry(*step_id)
                    .or_default()
                    .push(line.clone());
            }

            BuildEvent::StepCacheHit { step_id, .. } => {
                if let Some(span) = self.step_spans.get(step_id) {
                    let name = self.step_names.get(step_id).map_or("?", String::as_str);
                    span.pb_set_style(&completed_style(self.color));
                    span.pb_set_message(&format!("{name}  (cached)"));
                }
                self.step_outcomes.insert(*step_id, StepOutcome::Cached);
                if let Some(root) = &self.root_span {
                    root.pb_inc(1);
                }
            }

            BuildEvent::StepEnd {
                step_id,
                exit_code,
                duration_ms,
                ..
            } => {
                let cancelled = *exit_code == 130;
                if *exit_code != 0 && !cancelled {
                    self.failed_steps.push((*step_id, *exit_code));
                    if let Some(span) = self.step_spans.get(step_id) {
                        let name = self.step_names.get(step_id).map_or("?", String::as_str);
                        span.pb_set_style(&failed_style(self.color));
                        span.pb_set_message(&format!("{name}  FAILED (exit {exit_code})"));
                    }
                } else if cancelled {
                    if let Some(span) = self.step_spans.get(step_id) {
                        let name = self.step_names.get(step_id).map_or("?", String::as_str);
                        span.pb_set_style(&completed_style(self.color));
                        span.pb_set_message(&format!("{name}  (cancelled)"));
                    }
                } else if let Some(span) = self.step_spans.get(step_id) {
                    let name = self.step_names.get(step_id).map_or("?", String::as_str);
                    let dur = format_duration(*duration_ms);
                    span.pb_set_style(&completed_style(self.color));
                    span.pb_set_message(&format!("{name}  ({dur})"));
                }

                let outcome = if *exit_code == 0 {
                    StepOutcome::Succeeded {
                        duration_ms: *duration_ms,
                    }
                } else if cancelled {
                    StepOutcome::Cancelled {
                        duration_ms: *duration_ms,
                    }
                } else {
                    StepOutcome::Failed {
                        duration_ms: *duration_ms,
                        exit_code: *exit_code,
                    }
                };
                self.step_outcomes.insert(*step_id, outcome);

                if let Some(root) = &self.root_span {
                    root.pb_inc(1);
                }
            }

            BuildEvent::BuildAccepted { build, watch_url } => {
                if let Some(url) = watch_url {
                    let n = build
                        .number
                        .map(|n| format!("#{n} "))
                        .unwrap_or_default();
                    let _ = writeln!(self.out, "build {n}\u{2192} {url}");
                }
            }

            BuildEvent::ChainFailed { .. } => {}

            BuildEvent::BuildEnd {
                exit_code,
                duration_ms,
            } => {
                self.step_spans.clear();
                self.root_span.take();

                self.print_step_summary();

                if *exit_code != 0 {
                    self.print_failure_report();
                    let dur = format_duration(*duration_ms);
                    let msg = format!("✗ Build failed in {dur}");
                    let _ = writeln!(
                        self.out,
                        "\n{}",
                        styled(&msg, Style::new().red().bold(), self.color)
                    );
                } else {
                    let dur = format_duration(*duration_ms);
                    let msg = format!("✓ Build succeeded in {dur}");
                    let _ = writeln!(
                        self.out,
                        "\n{}",
                        styled(&msg, Style::new().green().bold(), self.color)
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;
    use hm_plugin_protocol::{PlanSummary, StdStream};

    fn renderer() -> ProgressRenderer<Vec<u8>> {
        ProgressRenderer::new(Vec::new(), false)
    }

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
            parent_key: None,
            display_name: "compile".into(),
        });

        r.on_event(&BuildEvent::StepLog {
            step_id,
            stream: StdStream::Stdout,
            line: "compiling main.rs".into(),
            ts: chrono::Utc::now(),
        });

        assert!(output(&r).is_empty(), "expected no text output");

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
            parent_key: None,
            display_name: "test".into(),
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
            parent_key: None,
            display_name: "build".into(),
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

        assert!(
            output(&r).contains("Build succeeded"),
            "expected success message on success: {:?}",
            output(&r)
        );
    }

    #[test]
    fn color_flag_stored() {
        let r = ProgressRenderer::new(Vec::<u8>::new(), true);
        assert!(r.color);
        let r2 = ProgressRenderer::new(Vec::<u8>::new(), false);
        assert!(!r2.color);
    }

    #[test]
    fn cache_hit_increments_root() {
        let mut r = renderer();
        let step_id = Uuid::new_v4();

        r.on_event(&BuildEvent::BuildStart {
            run_id: Uuid::nil(),
            plan: PlanSummary {
                step_count: 2,
                chain_count: 1,
                default_runner: "docker".into(),
            },
            started_at: chrono::Utc::now(),
        });

        r.on_event(&BuildEvent::StepQueued {
            step_id,
            key: "cached-step".into(),
            chain_idx: 0,
            parent_key: None,
            display_name: "cached-step".into(),
        });

        r.on_event(&BuildEvent::StepCacheHit {
            step_id,
            key: "cache-key".into(),
            tag: "img:tag".into(),
        });

        assert!(
            r.step_spans.contains_key(&step_id),
            "cached step span should stay alive"
        );
    }

    #[test]
    fn step_outcome_tracks_failure() {
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
            parent_key: None,
            display_name: "test".into(),
        });
        r.on_event(&BuildEvent::StepEnd {
            step_id,
            exit_code: 1,
            duration_ms: 500,
            snapshot: None,
        });

        assert!(
            matches!(
                r.step_outcomes.get(&step_id),
                Some(StepOutcome::Failed { exit_code: 1, .. })
            ),
            "expected Failed outcome"
        );
    }

    #[test]
    fn colored_summary_has_indicators() {
        let mut r = ProgressRenderer::new(Vec::new(), true);
        let s1 = Uuid::new_v4();
        let s2 = Uuid::new_v4();

        r.on_event(&BuildEvent::BuildStart {
            run_id: Uuid::nil(),
            plan: PlanSummary {
                step_count: 2,
                chain_count: 1,
                default_runner: "docker".into(),
            },
            started_at: chrono::Utc::now(),
        });
        r.on_event(&BuildEvent::StepQueued {
            step_id: s1,
            key: "build".into(),
            chain_idx: 0,
            parent_key: None,
            display_name: "build".into(),
        });
        r.on_event(&BuildEvent::StepEnd {
            step_id: s1,
            exit_code: 0,
            duration_ms: 200,
            snapshot: None,
        });
        r.on_event(&BuildEvent::StepQueued {
            step_id: s2,
            key: "test".into(),
            chain_idx: 0,
            parent_key: None,
            display_name: "test".into(),
        });
        r.on_event(&BuildEvent::StepEnd {
            step_id: s2,
            exit_code: 1,
            duration_ms: 300,
            snapshot: None,
        });
        r.on_event(&BuildEvent::BuildEnd {
            exit_code: 1,
            duration_ms: 600,
        });

        let s = output(&r);
        assert!(
            s.contains("\x1b[32m") && s.contains("✓"),
            "expected green ✓: {s}"
        );
        assert!(
            s.contains("\x1b[31m") && s.contains("✗"),
            "expected red ✗: {s}"
        );
        assert!(s.contains("Build failed"), "expected failure banner: {s}");
    }

    #[test]
    fn colored_success_banner() {
        let mut r = ProgressRenderer::new(Vec::new(), true);
        let s1 = Uuid::new_v4();

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
            step_id: s1,
            key: "build".into(),
            chain_idx: 0,
            parent_key: None,
            display_name: "build".into(),
        });
        r.on_event(&BuildEvent::StepEnd {
            step_id: s1,
            exit_code: 0,
            duration_ms: 100,
            snapshot: None,
        });
        r.on_event(&BuildEvent::BuildEnd {
            exit_code: 0,
            duration_ms: 150,
        });

        let s = output(&r);
        assert!(
            s.contains("\x1b[") && s.contains("Build succeeded"),
            "expected green bold success: {s}"
        );
        assert!(s.contains("Build succeeded"), "expected success: {s}");
    }
}
