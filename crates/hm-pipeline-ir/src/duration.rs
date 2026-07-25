//! A wire-friendly duration scalar.

use std::fmt;
use std::time::Duration;

use schemars::JsonSchema as DeriveJsonSchema;
use serde::{Deserialize, Serialize};

/// A duration in whole milliseconds, carried on the wire as a bare JSON number.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, DeriveJsonSchema,
)]
#[serde(transparent)]
pub struct DurationMs(pub u64);

impl DurationMs {
    /// The millisecond count.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Rebuild a [`Duration`] for computation or display.
    #[must_use]
    pub const fn as_duration(self) -> Duration {
        Duration::from_millis(self.0)
    }
}

impl From<Duration> for DurationMs {
    /// Saturating: a duration beyond `u64::MAX` ms (~584 million years) clamps to
    /// `u64::MAX` rather than panicking. `Duration::as_millis` returns `u128`, so
    /// the narrowing is real even though the ceiling cannot occur in practice.
    fn from(d: Duration) -> Self {
        Self(u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
    }
}

impl fmt::Display for DurationMs {
    /// The bare millisecond count (no unit suffix), matching the wire form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test setup and assertions")]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    #[case::zero(0, 0)]
    #[case::one(1, 1)]
    #[case::typical(1_500, 1_500)]
    fn from_duration_preserves_millis(#[case] millis: u64, #[case] expected: u64) {
        assert_eq!(DurationMs::from(Duration::from_millis(millis)).get(), expected);
    }

    #[rstest]
    fn from_duration_saturates_at_u64_max() {
        // A duration whose millisecond count exceeds u64::MAX clamps rather than
        // wrapping or panicking. `Duration::MAX` is ~5.85e12 seconds * 1000 ms,
        // well past u64::MAX ms.
        assert_eq!(DurationMs::from(Duration::MAX), DurationMs(u64::MAX));
    }

    #[rstest]
    fn as_duration_round_trips() {
        let d = DurationMs(1_234);
        assert_eq!(d.as_duration(), Duration::from_millis(1_234));
    }

    #[rstest]
    #[case::zero(DurationMs(0), "0")]
    #[case::typical(DurationMs(1_500), "1500")]
    fn display_is_bare_integer(#[case] d: DurationMs, #[case] expected: &str) {
        assert_eq!(d.to_string(), expected);
    }

    #[rstest]
    fn serializes_as_bare_json_number() {
        // `#[serde(transparent)]` means the wire form is a plain number, not an
        // object — the whole point of the newtype for cross-language consumers.
        let json = serde_json::to_string(&DurationMs(1_500)).unwrap();
        assert_eq!(json, "1500");
        let back: DurationMs = serde_json::from_str(&json).unwrap();
        assert_eq!(back, DurationMs(1_500));
    }
}
