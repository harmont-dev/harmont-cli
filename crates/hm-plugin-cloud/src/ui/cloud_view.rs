//! Live multi-job view: one spinner bar per running job, interleaved log lines
//! printed above via the shared MultiProgress, and per-job finish glyphs.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use uuid::Uuid;

const TICKS: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠀",
];

/// Per-job live progress bars, sharing one `MultiProgress` with the reporter.
#[derive(Debug)]
pub struct CloudJobView {
    multi: MultiProgress,
    bars: HashMap<Uuid, (ProgressBar, Instant)>,
    color: bool,
}

impl CloudJobView {
    /// Share the reporter's `MultiProgress` so lines and bars never collide.
    #[must_use]
    pub fn new(multi: MultiProgress, color: bool) -> Self {
        Self {
            multi,
            bars: HashMap::new(),
            color,
        }
    }

    fn spinner_style(&self) -> ProgressStyle {
        let tpl = if self.color {
            "{spinner:.yellow} {prefix:.bold} {wide_msg:.dim} ({elapsed})"
        } else {
            "{spinner} {prefix} {wide_msg} ({elapsed})"
        };
        ProgressStyle::with_template(tpl)
            .unwrap_or_else(|_| ProgressStyle::default_spinner())
            .tick_strings(TICKS)
    }

    /// A job transitions to running: add a live spinner bar (idempotent).
    pub fn job_running(&mut self, id: Uuid, name: &str) {
        if self.bars.contains_key(&id) {
            return;
        }
        let pb = self.multi.add(ProgressBar::new_spinner());
        pb.set_style(self.spinner_style());
        pb.set_prefix(name.to_string());
        pb.set_message("running");
        pb.enable_steady_tick(Duration::from_millis(80));
        self.bars.insert(id, (pb, Instant::now()));
    }

    /// A streaming log line for a job — printed above the live bars.
    pub fn job_log(&self, id: Uuid, name: &str, line: &str) {
        let prefix = if self.color {
            format!("{name} |").dimmed().to_string()
        } else {
            format!("{name} |")
        };
        if let Some((pb, _)) = self.bars.get(&id) {
            pb.println(format!("{prefix} {line}"));
        } else {
            let _ = self.multi.println(format!("{prefix} {line}"));
        }
    }

    /// A job reaches a terminal state: finish its bar in place with a glyph.
    pub fn job_done(&mut self, id: Uuid, name: &str, passed: bool) {
        if let Some((pb, started)) = self.bars.remove(&id) {
            let secs = started.elapsed().as_secs_f64();
            let glyph = match (passed, self.color) {
                (true, true) => "✓".green().to_string(),
                (false, true) => "✗".red().to_string(),
                (true, false) => "✓".to_string(),
                (false, false) => "✗".to_string(),
            };
            if let Ok(style) = ProgressStyle::with_template("{msg}") {
                pb.set_style(style);
            }
            pb.finish_with_message(format!("{glyph} {name}  ({secs:.1}s)"));
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn cloud_job_view_state_transitions_without_panic() {
        let multi = MultiProgress::new();
        let mut view = CloudJobView::new(multi, false);
        let id = Uuid::new_v4();

        // job_log before job_running falls through to the multi path
        view.job_log(id, "pre-run", "early line");

        view.job_running(id, "test-job");
        assert!(view.bars.contains_key(&id), "bar inserted on job_running");

        // idempotent second call must not insert a duplicate
        view.job_running(id, "test-job");
        assert_eq!(view.bars.len(), 1, "idempotent job_running");

        view.job_log(id, "test-job", "step 1 output");
        view.job_log(id, "test-job", "step 2 output");

        view.job_done(id, "test-job", true);
        assert!(!view.bars.contains_key(&id), "bar removed after job_done");
        assert!(view.bars.is_empty(), "no lingering bars");
    }

    #[test]
    fn cloud_job_view_failed_job_done_color() {
        let multi = MultiProgress::new();
        let mut view = CloudJobView::new(multi, true);
        let id = Uuid::new_v4();
        view.job_running(id, "failing-job");
        view.job_done(id, "failing-job", false);
        assert!(view.bars.is_empty());
    }

    #[test]
    fn cloud_job_view_done_on_unknown_id_is_noop() {
        let multi = MultiProgress::new();
        let mut view = CloudJobView::new(multi, false);
        // Should not panic when the id was never registered.
        view.job_done(Uuid::new_v4(), "ghost-job", true);
    }
}
