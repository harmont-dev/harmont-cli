//! Live multi-job view: one spinner bar per running job, interleaved log lines
//! printed above via the shared MultiProgress, and per-job finish glyphs.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use owo_colors::OwoColorize;
use uuid::Uuid;

const TICKS: &[&str] = &[
    "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏", "⠀",
];

/// How a `CloudJobView` renders job lifecycle + logs.
#[derive(Clone)]
enum ViewMode {
    /// Interactive (TTY): live steady-tick spinner bars per running job.
    Interactive,
    /// Plain (non-TTY/CI): prefixed log lines + glyphs, no animated bars.
    Plain,
    /// JSON (`--format json`): one compact JSON object per event to the writer.
    Json(Arc<Mutex<dyn Write + Send>>),
}

impl std::fmt::Debug for ViewMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Interactive => f.write_str("Interactive"),
            Self::Plain => f.write_str("Plain"),
            Self::Json(_) => f.write_str("Json"),
        }
    }
}

/// Per-job live progress bars, sharing one `MultiProgress` with the reporter.
#[derive(Debug)]
pub struct CloudJobView {
    multi: MultiProgress,
    bars: HashMap<Uuid, (ProgressBar, Instant)>,
    color: bool,
    /// Render mode. Plain/JSON never create steady-tick spinner bars, so no
    /// animated frames leak into a pipe, log file, or JSON stream.
    mode: ViewMode,
}

impl CloudJobView {
    /// Share the reporter's `MultiProgress` so lines and bars never collide.
    ///
    /// Use this for the interactive (TTY) path, where live spinner bars are
    /// drawn above streamed log lines.
    #[must_use]
    pub fn new(multi: MultiProgress, color: bool) -> Self {
        Self {
            multi,
            bars: HashMap::new(),
            color,
            mode: ViewMode::Interactive,
        }
    }

    /// A plain (non-TTY/CI) view: no animated spinners, just prefixed log lines
    /// and terminal-state glyphs printed straight to `multi` via `println`.
    ///
    /// Pass a `MultiProgress` whose draw target is the desired sink (e.g.
    /// `MultiProgress::new()` writing to stderr); in plain mode no bars are
    /// added, so nothing animates.
    #[must_use]
    pub fn plain(multi: MultiProgress, color: bool) -> Self {
        Self {
            multi,
            bars: HashMap::new(),
            color,
            mode: ViewMode::Plain,
        }
    }

    /// A JSON view (`--format json`): emit one compact JSON object per event to
    /// `out` (typically `std::io::stdout()`), no animated bars. Job lifecycle
    /// transitions and log lines become machine-readable records:
    /// `{"type":"job",...}` and `{"type":"log",...}`.
    #[must_use]
    pub fn json<W: Write + Send + 'static>(out: W) -> Self {
        Self {
            // A hidden MultiProgress so no spinner frames are ever drawn; all
            // output goes through the JSON writer below.
            multi: MultiProgress::with_draw_target(indicatif::ProgressDrawTarget::hidden()),
            bars: HashMap::new(),
            color: false,
            mode: ViewMode::Json(Arc::new(Mutex::new(out))),
        }
    }

    /// Emit one compact JSON object to the JSON sink (no-op if not JSON mode).
    fn emit_json(&self, obj: &serde_json::Value) {
        if let ViewMode::Json(out) = &self.mode
            && let Ok(mut w) = out.lock()
        {
            let _ = writeln!(w, "{obj}");
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
    ///
    /// In plain mode this prints a single status line and adds no animated bar.
    pub fn job_running(&mut self, id: Uuid, name: &str) {
        if let ViewMode::Json(_) = self.mode {
            self.emit_json(&serde_json::json!({
                "type": "job", "state": "running",
                "job_id": id.to_string(), "name": name,
            }));
            return;
        }
        if matches!(self.mode, ViewMode::Plain) {
            let line = if self.color {
                format!("{name} | running").dimmed().to_string()
            } else {
                format!("{name} | running")
            };
            let _ = self.multi.println(line);
            return;
        }
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
        if let ViewMode::Json(_) = self.mode {
            self.emit_json(&serde_json::json!({
                "type": "log",
                "job_id": id.to_string(), "name": name, "line": line,
            }));
            return;
        }
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
    ///
    /// In plain mode this prints a single terminal-state line (no bar exists).
    pub fn job_done(&mut self, id: Uuid, name: &str, passed: bool) {
        if let ViewMode::Json(_) = self.mode {
            self.emit_json(&serde_json::json!({
                "type": "job",
                "state": if passed { "passed" } else { "failed" },
                "job_id": id.to_string(), "name": name,
            }));
            return;
        }
        if matches!(self.mode, ViewMode::Plain) {
            let glyph = match (passed, self.color) {
                (true, true) => "✓".green().to_string(),
                (false, true) => "✗".red().to_string(),
                (true, false) => "✓".to_string(),
                (false, false) => "✗".to_string(),
            };
            let _ = self.multi.println(format!("{glyph} {name}"));
            return;
        }
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

    #[test]
    fn plain_view_never_creates_spinner_bars() {
        // Plain mode (non-TTY/CI): no animated bars are ever added, so the
        // bars map stays empty across the whole lifecycle. This guarantees no
        // steady-tick spinner frames can leak into a pipe or log file.
        let multi = MultiProgress::new();
        let mut view = CloudJobView::plain(multi, false);
        let id = Uuid::new_v4();

        view.job_running(id, "test-job");
        assert!(view.bars.is_empty(), "plain mode adds no bars");

        view.job_log(id, "test-job", "line of output");
        view.job_done(id, "test-job", true);
        assert!(view.bars.is_empty(), "plain mode still has no bars");

        // A failed job in color mode also stays bar-free.
        let mut color_view = CloudJobView::plain(MultiProgress::new(), true);
        color_view.job_running(id, "fail-job");
        color_view.job_done(id, "fail-job", false);
        assert!(color_view.bars.is_empty());
    }

    // A Write that shares a Vec so the test can inspect JSON output.
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);
    impl Write for SharedBuf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn json_view_emits_parseable_records() {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let mut view = CloudJobView::json(SharedBuf(buf.clone()));
        let id = Uuid::new_v4();

        view.job_running(id, "build");
        view.job_log(id, "build", "compiling…");
        view.job_done(id, "build", true);

        // JSON mode never creates animated bars.
        assert!(view.bars.is_empty(), "json mode adds no bars");

        let out = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        let mut lines = out.lines();

        let running: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(running["type"], "job");
        assert_eq!(running["state"], "running");
        assert_eq!(running["job_id"], id.to_string());
        assert_eq!(running["name"], "build");

        let log: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(log["type"], "log");
        assert_eq!(log["line"], "compiling…");
        assert_eq!(log["job_id"], id.to_string());

        let done: serde_json::Value = serde_json::from_str(lines.next().unwrap()).unwrap();
        assert_eq!(done["type"], "job");
        assert_eq!(done["state"], "passed");
    }

    #[test]
    fn json_view_done_failed_state() {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let mut view = CloudJobView::json(SharedBuf(buf.clone()));
        let id = Uuid::new_v4();
        view.job_done(id, "t", false);
        let out = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        let v: serde_json::Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
        assert_eq!(v["state"], "failed");
    }
}
