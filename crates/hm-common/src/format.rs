//! Formatting utilities.

use core::fmt::{self, Display};
use core::time::Duration;

mod sealed {
    pub trait Sealed {}
    impl Sealed for core::time::Duration {}
}

/// Extension trait adding a compact, stopwatch-style [`Display`] rendering to
/// [`std::time::Duration`].
///
/// ```
/// # use hm_common::format::CompactDuration;
/// # use std::time::Duration;
/// assert_eq!(Duration::from_millis(3_661_000).compact().to_string(), "1h1m1s");
/// ```
pub trait CompactDuration: sealed::Sealed {
    /// Wrap `self` in a [`StopwatchDurationDisplay`] for compact rendering.
    fn compact(self) -> StopwatchDurationDisplay;
}

impl CompactDuration for Duration {
    fn compact(self) -> StopwatchDurationDisplay {
        StopwatchDurationDisplay {
            total_ms: u64::try_from(self.as_millis()).unwrap_or(u64::MAX),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StopwatchDurationDisplay {
    total_ms: u64,
}

impl Display for StopwatchDurationDisplay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [(&str, u64); 5] = [
            ("d", 1000 * 60 * 60 * 24),
            ("h", 1000 * 60 * 60),
            ("m", 1000 * 60),
            ("s", 1000),
            ("ms", 1),
        ];

        if self.total_ms == 0 {
            return f.write_str("0s");
        }
        let mut rem = self.total_ms;
        for (suffix, scale) in UNITS {
            let n = rem / scale;
            rem %= scale;
            if n > 0 {
                write!(f, "{n}{suffix}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::panic, reason = "test helpers assert on well-formed input")]
mod tests {
    use super::*;
    use proptest::prelude::*;
    use rstest::rstest;

    fn render(ms: u64) -> String {
        Duration::from_millis(ms).compact().to_string()
    }

    #[rstest]
    #[case(0, "0s")]
    #[case(5, "5ms")]
    #[case(500, "500ms")]
    #[case(999, "999ms")]
    #[case(1_000, "1s")]
    #[case(1_050, "1s50ms")]
    #[case(1_500, "1s500ms")]
    #[case(60_000, "1m")]
    #[case(61_000, "1m1s")]
    #[case(90_000, "1m30s")]
    #[case(123_456, "2m3s456ms")]
    #[case(3_600_000, "1h")]
    #[case(3_661_000, "1h1m1s")]
    #[case(7_200_000, "2h")]
    #[case(86_400_000, "1d")]
    #[case(93_600_000, "1d2h")]
    fn renders_expected(#[case] ms: u64, #[case] expected: &str) {
        assert_eq!(render(ms), expected);
    }

    /// Sum the components back out of a rendered string. Mirrors nothing in the
    /// implementation, so it is an independent check of losslessness.
    fn parse_back(s: &str) -> u64 {
        let b = s.as_bytes();
        let mut i = 0;
        let mut total = 0u64;
        while i < b.len() {
            let mut n = 0u64;
            while i < b.len() && b[i].is_ascii_digit() {
                n = n * 10 + u64::from(b[i] - b'0');
                i += 1;
            }
            // `ms` before the bare `m`/`s` cases so it is matched first.
            let scale = if b[i] == b'm' && i + 1 < b.len() && b[i + 1] == b's' {
                i += 2;
                1
            } else {
                let unit = b[i];
                i += 1;
                match unit {
                    b's' => 1_000,
                    b'm' => 1_000 * 60,
                    b'h' => 1_000 * 60 * 60,
                    b'd' => 1_000 * 60 * 60 * 24,
                    other => panic!("unexpected unit byte {other}"),
                }
            };
            total += n * scale;
        }
        total
    }

    proptest! {
        /// Never panics, never empty, uses only the expected glyphs, and ends
        /// on a unit letter — for the entire `u64` millisecond range.
        #[test]
        fn well_formed_over_full_range(ms in any::<u64>()) {
            let out = render(ms);
            prop_assert!(!out.is_empty());
            prop_assert!(
                out.bytes().all(|c| c.is_ascii_digit() || matches!(c, b'd' | b'h' | b'm' | b's')),
                "unexpected glyph in {out:?}"
            );
            prop_assert!(matches!(out.bytes().last(), Some(b'd' | b'h' | b'm' | b's')));
        }

        /// humantime semantics are lossless: the rendered components sum back to
        /// the exact input (bounded below 30 days, where our unit ladder — up to
        /// days — matches a full decomposition with no rollover to months).
        #[test]
        fn round_trips_losslessly(ms in 0u64..30 * 24 * 60 * 60 * 1_000) {
            prop_assert_eq!(parse_back(&render(ms)), ms);
        }
    }
}
