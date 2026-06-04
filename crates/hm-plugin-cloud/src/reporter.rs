//! The single output chokepoint for cloud commands. Nothing else writes to a
//! terminal. Satisfies clippy::print_stdout/print_stderr because it uses
//! `Write`/`writeln!` (and, in F2, `MultiProgress::println`) — never the
//! `println!`/`eprintln!` macros.

use std::io::Write;
use std::sync::Mutex;

use indicatif::MultiProgress;
use owo_colors::OwoColorize;

/// Severity of a status line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
    Success,
}

/// Output sink for cloud commands.
pub trait Reporter: Send + Sync {
    /// A durable transcript line (survives in scrollback / `tee`).
    fn line(&self, text: &str);
    /// A semantic status line (tagged + colored by level).
    fn status(&self, level: Level, text: &str);
}

/// Format a status line: a level glyph + text, colored iff `color`.
pub(crate) fn format_status(level: Level, text: &str, color: bool) -> String {
    let tag = match level {
        Level::Info => "·",
        Level::Warn => "!",
        Level::Error => "✗",
        Level::Success => "✓",
    };
    if color {
        match level {
            Level::Error => format!("{} {text}", tag.red()),
            Level::Warn => format!("{} {text}", tag.yellow()),
            Level::Success => format!("{} {text}", tag.green()),
            Level::Info => format!("{} {text}", tag.dimmed()),
        }
    } else {
        format!("{tag} {text}")
    }
}

/// Plain / CI / non-TTY reporter: writes straight to an inner writer.
/// Color is applied to status tags only when `color` is true.
#[derive(Debug)]
pub struct PlainReporter<W: Write + Send> {
    out: Mutex<W>,
    color: bool,
}

impl<W: Write + Send> PlainReporter<W> {
    /// Create a new `PlainReporter` writing to `out`.
    ///
    /// Pass `color = true` to emit ANSI escape codes on status tags.
    pub fn new(out: W, color: bool) -> Self {
        Self {
            out: Mutex::new(out),
            color,
        }
    }
}

impl<W: Write + Send> Reporter for PlainReporter<W> {
    fn line(&self, text: &str) {
        if let Ok(mut w) = self.out.lock() {
            let _ = writeln!(w, "{text}");
        }
    }

    fn status(&self, level: Level, text: &str) {
        self.line(&format_status(level, text, self.color));
    }
}

/// Interactive TTY reporter. Holds the `MultiProgress` so log lines print
/// *above* live job bars via `MultiProgress::println` (never corrupting them).
/// Lint-clean: `MultiProgress::println` is a method, not the `println!` macro.
#[derive(Clone, Debug)]
pub struct TermReporter {
    multi: MultiProgress,
    color: bool,
}

impl TermReporter {
    /// Create a new `TermReporter`.
    ///
    /// Pass `color = true` to emit ANSI escape codes.
    pub fn new(color: bool) -> Self {
        Self {
            multi: MultiProgress::new(),
            color,
        }
    }

    /// Share the `MultiProgress` with a `CloudJobView` so bars and lines coexist.
    pub fn multi(&self) -> MultiProgress {
        self.multi.clone()
    }

    /// Whether ANSI color output is enabled.
    pub fn color(&self) -> bool {
        self.color
    }
}

impl Reporter for TermReporter {
    fn line(&self, text: &str) {
        let _ = self.multi.println(text);
    }

    fn status(&self, level: Level, text: &str) {
        let _ = self.multi.println(format_status(level, text, self.color));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    // A Write that shares a Vec so the test can inspect output.
    struct SharedBuf(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for SharedBuf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn plain_reporter_writes_lines_without_ansi_when_color_off() {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let r = PlainReporter::new(SharedBuf(buf.clone()), false);
        r.line("hello");
        r.status(Level::Error, "boom");
        let out = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(out.contains("hello"));
        assert!(out.contains("boom"));
        assert!(!out.contains('\u{1b}'), "no ANSI escapes when color=false");
    }

    #[test]
    fn plain_reporter_colors_when_enabled() {
        let buf = Arc::new(Mutex::new(Vec::<u8>::new()));
        let r = PlainReporter::new(SharedBuf(buf.clone()), true);
        r.status(Level::Error, "boom");
        let out = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(out.contains("boom"));
        assert!(out.contains('\u{1b}'), "ANSI escape present when color=true");
    }

    #[test]
    fn term_reporter_constructs_and_does_not_panic() {
        let r = TermReporter::new(false);
        r.line("hello from term");
        r.status(Level::Info, "all good");
        r.status(Level::Warn, "watch out");
        r.status(Level::Error, "something failed");
        r.status(Level::Success, "done");
        // color=true path
        let rc = TermReporter::new(true);
        rc.status(Level::Error, "colored error");
        rc.status(Level::Success, "colored success");
    }

    #[test]
    fn term_reporter_multi_is_cloneable() {
        let r = TermReporter::new(false);
        let _m = r.multi();
        assert!(!r.color());
        let rc = TermReporter::new(true);
        assert!(rc.color());
    }
}
