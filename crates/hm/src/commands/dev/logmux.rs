//! Multi-source line-prefixed colored log stream.

use std::io::Write;

use owo_colors::{AnsiColors, OwoColorize};
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone)]
pub struct LogLine {
    pub slug: String,
    pub bytes: Vec<u8>,
}

/// Per-slug line buffer: docker streams chunks that may not be
/// line-aligned. We accumulate bytes per slug and flush on each \n.
#[derive(Default)]
struct PerSlug {
    buf: Vec<u8>,
}

impl PerSlug {
    fn ingest<W: Write>(
        &mut self,
        slug: &str,
        width: usize,
        color: bool,
        bytes: &[u8],
        w: &mut W,
    ) -> std::io::Result<()> {
        self.buf.extend_from_slice(bytes);
        while let Some(idx) = self.buf.iter().position(|&b| b == b'\n') {
            // line = bytes up to (excluding) the newline
            let line = &self.buf[..idx];
            write_line(slug, width, color, line, w)?;
            self.buf.drain(..=idx);
        }
        Ok(())
    }

    fn flush<W: Write>(
        &mut self,
        slug: &str,
        width: usize,
        color: bool,
        w: &mut W,
    ) -> std::io::Result<()> {
        if !self.buf.is_empty() {
            let line = std::mem::take(&mut self.buf);
            write_line(slug, width, color, &line, w)?;
        }
        Ok(())
    }
}

fn slug_color(slug: &str) -> AnsiColors {
    // Stable color per slug via hash. 6 ANSI colors cycled.
    const PALETTE: [AnsiColors; 6] = [
        AnsiColors::Cyan,
        AnsiColors::Magenta,
        AnsiColors::Yellow,
        AnsiColors::Green,
        AnsiColors::Blue,
        AnsiColors::BrightRed,
    ];
    let mut h: u32 = 0;
    for b in slug.bytes() {
        h = h.wrapping_mul(31).wrapping_add(u32::from(b));
    }
    PALETTE[(h as usize) % PALETTE.len()]
}

fn write_line<W: Write>(
    slug: &str,
    width: usize,
    color: bool,
    line: &[u8],
    w: &mut W,
) -> std::io::Result<()> {
    let prefix = format!("[{slug:<width$}]");
    if color {
        write!(w, "{} ", prefix.color(slug_color(slug)))?;
    } else {
        write!(w, "{prefix} ")?;
    }
    w.write_all(line)?;
    w.write_all(b"\n")?;
    Ok(())
}

/// Consume `LogLine` messages, write `[slug] line\n` to stdout per line.
///
/// `slug_width` is the column width for the slug prefix; pass the
/// length of the longest slug in this session so columns align.
/// `color` toggles ANSI coloring.
///
/// Returns when the channel closes.
///
/// # Errors
///
/// Returns an `std::io::Error` if writing to stdout fails.
pub async fn run(
    mut rx: UnboundedReceiver<LogLine>,
    slug_width: usize,
    color: bool,
) -> std::io::Result<()> {
    use std::collections::HashMap;
    use std::io::Write;
    let mut buffers: HashMap<String, PerSlug> = HashMap::new();
    while let Some(msg) = rx.recv().await {
        // Accumulate into a temporary Vec so we don't hold StdoutLock
        // (non-Send) across the await point above.
        let mut tmp: Vec<u8> = Vec::new();
        let entry = buffers.entry(msg.slug.clone()).or_default();
        entry.ingest(&msg.slug, slug_width, color, &msg.bytes, &mut tmp)?;
        if !tmp.is_empty() {
            let mut stdout = std::io::stdout().lock();
            stdout.write_all(&tmp)?;
        }
    }
    // Final flush — also into tmp first, then stdout.
    let mut tmp: Vec<u8> = Vec::new();
    for (slug, mut b) in buffers {
        b.flush(&slug, slug_width, color, &mut tmp)?;
    }
    if !tmp.is_empty() {
        let mut stdout = std::io::stdout().lock();
        stdout.write_all(&tmp)?;
        stdout.flush()?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    fn capture(slug: &str, chunks: &[&[u8]], color: bool) -> String {
        let mut buf: Vec<u8> = Vec::new();
        let mut p = PerSlug::default();
        for c in chunks {
            p.ingest(slug, 4, color, c, &mut buf).unwrap();
        }
        p.flush(slug, 4, color, &mut buf).unwrap();
        String::from_utf8(buf).unwrap()
    }

    #[test]
    fn flushes_on_newline() {
        let out = capture("db", &[b"hello\n"], false);
        assert_eq!(out, "[db  ] hello\n");
    }

    #[test]
    fn buffers_partial_chunk_across_calls() {
        let out = capture("db", &[b"hel", b"lo\nworld\n"], false);
        assert_eq!(out, "[db  ] hello\n[db  ] world\n");
    }

    #[test]
    fn flush_emits_trailing_unterminated_line() {
        let out = capture("db", &[b"tail"], false);
        assert_eq!(out, "[db  ] tail\n");
    }

    #[test]
    fn color_wraps_prefix_with_ansi() {
        let out = capture("db", &[b"hi\n"], true);
        assert!(out.contains("hi"));
        // ANSI escape introducer
        assert!(out.contains("\x1b["));
    }

    #[test]
    fn slug_color_is_stable_per_slug() {
        assert_eq!(slug_color("db"), slug_color("db"));
        // Different slugs *probably* get different colors; not asserted.
    }
}
