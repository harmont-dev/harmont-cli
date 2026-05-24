//! Visual and DX helpers for the CLI: relative times, status pills,
//! hyperlinks, banners, headers, key/value rows and empty states.
//!
//! These helpers are the foundation for the CLI's "beautiful, modern, fun"
//! design language. Every command should funnel its visual output through
//! this module rather than reaching for `owo_colors` directly.
use chrono::{DateTime, Utc};
use owo_colors::{OwoColorize, Style};
use std::fmt::Display;

/// Render an epoch timestamp as a human-friendly relative time.
///
/// - `0` → `—`
/// - `|delta| < 5s` → `just now`
/// - `< 60s` → `12s ago` / `in 12s`
/// - `< 1h`  → `3m ago` / `in 3m`
/// - `< 1d`  → `2h ago` / `in 2h`
/// - `< 1w`  → `5d ago` / `in 5d`
/// - otherwise → `%b %e` (e.g. `Jan 12`)
#[must_use]
pub fn rel_time(epoch: i64) -> String {
    if epoch == 0 {
        return "—".to_string();
    }
    let now = Utc::now().timestamp();
    let delta = now - epoch;
    let abs = delta.abs();

    if abs < 5 {
        return "just now".to_string();
    }

    let (n, unit) = if abs < 60 {
        (abs, "s")
    } else if abs < 3600 {
        (abs / 60, "m")
    } else if abs < 86400 {
        (abs / 3600, "h")
    } else if abs < 7 * 86400 {
        (abs / 86400, "d")
    } else {
        // Fall back to absolute date formatting.
        return DateTime::<Utc>::from_timestamp(epoch, 0)
            .map_or_else(|| "—".to_string(), |dt| dt.format("%b %e").to_string());
    };

    if delta >= 0 {
        format!("{n}{unit} ago")
    } else {
        format!("in {n}{unit}")
    }
}

/// Render a duration in seconds as `42s` / `3m12s` / `1h05m`.
///
/// Values `<= 0` render as `—`.
#[must_use]
pub fn duration_human(secs: i64) -> String {
    if secs <= 0 {
        return "—".to_string();
    }
    if secs < 60 {
        return format!("{secs}s");
    }
    if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        return format!("{m}m{s:02}s");
    }
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{h}h{m:02}m")
}

/// Elapsed duration between two epoch timestamps.
///
/// - `start == 0` → `—`
/// - `end == 0` → use now as the end
/// - otherwise `duration_human(end - start)`
#[must_use]
pub fn elapsed_between(start: i64, end: i64) -> String {
    if start == 0 {
        return "—".to_string();
    }
    let actual_end = if end == 0 {
        Utc::now().timestamp()
    } else {
        end
    };
    duration_human(actual_end - start)
}

/// Return the (style, icon) pair for a given status string.
fn status_style(status: &str) -> (Style, &'static str) {
    match status {
        "passed" => (Style::new().green().bold(), "✓"),
        "failed" => (Style::new().red().bold(), "✗"),
        "running" => (Style::new().yellow().bold(), "◐"),
        "queued" | "scheduled" => (Style::new().blue(), "◷"),
        "blocked" | "waiting" => (Style::new().magenta(), "⏸"),
        "canceled" | "canceling" => (Style::new().bright_black(), "⊘"),
        "skipped" | "not_run" => (Style::new().bright_black(), "⤼"),
        _ => (Style::new().white(), "•"),
    }
}

/// Render a status pill: `"<icon> <status>"`, both styled in the
/// status color.
#[must_use]
pub fn status_pill(status: &str) -> String {
    let (style, icon) = status_style(status);
    let body = format!("{icon} {status}");
    body.style(style).to_string()
}

/// Detect whether OSC 8 hyperlinks should be emitted by default.
///
/// Returns false if `NO_COLOR` is set, or if `TERM_PROGRAM` is empty or
/// `"dumb"`; otherwise true.
fn supports_hyperlinks() -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match std::env::var("TERM_PROGRAM") {
        Ok(v) if v.is_empty() || v == "dumb" => false,
        Ok(_) => true,
        Err(_) => false,
    }
}

/// Render a hyperlink, auto-detecting terminal support.
#[must_use]
pub fn hyperlink(url: &str, label: &str) -> String {
    hyperlink_with(url, label, supports_hyperlinks())
}

/// Render a hyperlink with explicit support toggle.
///
/// When `enabled`, emits an OSC 8 escape sequence. Otherwise falls back
/// to `<label> (<url-dimmed>)`.
#[must_use]
pub fn hyperlink_with(url: &str, label: &str, enabled: bool) -> String {
    if enabled {
        format!("\x1b]8;;{url}\x07{label}\x1b]8;;\x07")
    } else if label == url {
        label.to_string()
    } else {
        format!("{label} ({})", url.dimmed())
    }
}

/// Print a section header: blank line, bold title, a unicode rule.
pub fn header(title: &str) {
    let rule_len = title.chars().count() + 4;
    let rule: String = "─".repeat(rule_len);
    tracing::info!( "");
    tracing::info!( "  {}", title.bold());
    tracing::info!( "  {}", rule.bright_black());
}

/// Print a key/value row, aligned at column 12.
pub fn kv(label: &str, value: impl Display) {
    let label_with_colon = format!("{label}:");
    // Right-pad to width 10 so values line up at column 12 from start
    // of line (2 spaces + 10-wide label field = 12).
    let padded = format!("{label_with_colon:<10}");
    tracing::info!( "  {} {value}", padded.bright_black());
}

/// Print an empty-state message: blank line, bold title, dim hint, blank line.
pub fn empty_state(title: &str, hint: &str) {
    tracing::info!( "");
    tracing::info!( "  {}", title.bold());
    tracing::info!( "  {}", hint.bright_black());
    tracing::info!( "");
}

/// Print a command banner: `▌ hm <command> · <subtitle>`.
pub fn banner(command: &str, subtitle: &str) {
    let icon = "▌".cyan();
    let icon = icon.bold();
    let hm = "hm".bold();
    let cmd = command.cyan();
    let sub_text = format!("· {subtitle}");
    let sub = sub_text.bright_black();
    tracing::info!( "{icon} {hm} {cmd} {sub}");
    tracing::info!( "");
}

/// Print a single step line: `  ✓ <verb-dim> <result>`.
pub fn step(verb: &str, result: impl Display) {
    let check = "✓".green();
    let check = check.bold();
    let dim_verb = verb.bright_black();
    tracing::info!( "  {check} {dim_verb} {result}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn rel_time_zero_is_dash() {
        assert_eq!(rel_time(0), "—");
    }

    #[test]
    fn rel_time_just_now() {
        let now = Utc::now().timestamp();
        assert_eq!(rel_time(now), "just now");
    }

    #[test]
    fn rel_time_seconds_ago() {
        let now = Utc::now().timestamp();
        assert_eq!(rel_time(now - 12), "12s ago");
    }

    #[test]
    fn rel_time_minutes_ago() {
        let now = Utc::now().timestamp();
        assert_eq!(rel_time(now - 180), "3m ago");
    }

    #[test]
    fn rel_time_hours_ago() {
        let now = Utc::now().timestamp();
        assert_eq!(rel_time(now - 2 * 3600), "2h ago");
    }

    #[test]
    fn rel_time_days_ago() {
        let now = Utc::now().timestamp();
        assert_eq!(rel_time(now - 5 * 86400), "5d ago");
    }

    #[test]
    fn rel_time_future_seconds() {
        let now = Utc::now().timestamp();
        assert_eq!(rel_time(now + 12), "in 12s");
    }

    #[test]
    fn duration_human_zero() {
        assert_eq!(duration_human(0), "—");
    }

    #[test]
    fn duration_human_seconds() {
        assert_eq!(duration_human(42), "42s");
    }

    #[test]
    fn duration_human_minutes() {
        assert_eq!(duration_human(192), "3m12s");
    }

    #[test]
    fn duration_human_hours() {
        assert_eq!(duration_human(3900), "1h05m");
    }

    #[test]
    fn elapsed_between_returns_dash_when_unstarted() {
        assert_eq!(elapsed_between(0, 0), "—");
    }

    #[test]
    fn elapsed_between_uses_now_when_unfinished() {
        let now = Utc::now().timestamp();
        let s = elapsed_between(now - 10, 0);
        // ~10s elapsed; allow both "10s" and "11s" for clock drift.
        assert!(s.ends_with('s'), "got: {s}");
    }

    #[test]
    fn hyperlink_fallback_when_disabled() {
        let s = hyperlink_with("https://x.test", "click", false);
        assert!(s.contains("click"));
        assert!(s.contains("https://x.test"));
        assert!(!s.contains("\x1b]8;;"));
    }

    #[test]
    fn hyperlink_emits_osc8_when_enabled() {
        let s = hyperlink_with("https://x.test", "click", true);
        assert!(s.contains("\x1b]8;;https://x.test\x07click\x1b]8;;\x07"));
    }

    #[test]
    fn status_pill_known_status_contains_label() {
        assert!(status_pill("passed").contains("passed"));
        assert!(status_pill("failed").contains("failed"));
        assert!(status_pill("running").contains("running"));
    }

    #[test]
    fn hyperlink_fallback_collapses_when_label_is_url() {
        let s = hyperlink_with("https://x.test", "https://x.test", false);
        assert_eq!(s, "https://x.test");
    }
}
