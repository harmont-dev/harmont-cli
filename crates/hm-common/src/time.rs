//! System-clock, epoch, and now-relative time helpers.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeDelta, Utc};

mod sealed {
    pub trait SealedDuration {}
    impl SealedDuration for std::time::Duration {}

    pub trait SealedDateTime {}
    impl SealedDateTime for chrono::DateTime<chrono::Utc> {}
}

/// Extension trait adding system-clock readings to [`Duration`].
pub trait DurationExt: sealed::SealedDuration {
    /// The current Unix time in whole seconds.
    ///
    /// Reads the system clock via [`SystemTime::now`], returning `0` if the
    /// clock is set before the Unix epoch. If the second count overflows `i64`
    /// — unreachable for ~292 billion years — this logs via [`tracing::error!`]
    /// and saturates at `i64::MAX` rather than wrapping.
    #[must_use]
    fn now_epoch_secs_i64() -> i64;
}

impl DurationExt for Duration {
    fn now_epoch_secs_i64() -> i64 {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        i64::try_from(secs).unwrap_or_else(|_| {
            tracing::error!(secs, "Unix epoch seconds overflowed i64; saturating at i64::MAX");
            i64::MAX
        })
    }
}

/// Extension trait adding now-relative readings to [`DateTime<Utc>`].
pub trait DateTimeExt: sealed::SealedDateTime {
    /// The signed duration from now until this instant: positive when it is in
    /// the future, negative when in the past. Equivalent to `self - Utc::now()`.
    #[must_use]
    fn time_from_now(self) -> TimeDelta;
}

impl DateTimeExt for DateTime<Utc> {
    fn time_from_now(self) -> TimeDelta {
        self - Utc::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    #[rstest]
    fn now_is_after_2020_and_before_2100() {
        let now = Duration::now_epoch_secs_i64();
        // 1_577_836_800 = 2020-01-01, 4_102_444_800 = 2100-01-01.
        assert!(now > 1_577_836_800, "epoch secs {now} implausibly small (< 2020)");
        assert!(now < 4_102_444_800, "epoch secs {now} implausibly large (> 2100)");
    }

    #[rstest]
    fn successive_reads_are_nondecreasing() {
        let a = Duration::now_epoch_secs_i64();
        let b = Duration::now_epoch_secs_i64();
        assert!(b >= a, "clock went backwards: {a} then {b}");
    }

    #[rstest]
    fn time_from_now_is_positive_for_a_future_instant() {
        let future = Utc::now() + TimeDelta::seconds(60);
        let d = future.time_from_now();
        assert!(d > TimeDelta::zero(), "expected positive, got {d}");
        assert!(d <= TimeDelta::seconds(60), "should be <= 60s, got {d}");
    }

    #[rstest]
    fn time_from_now_is_negative_for_a_past_instant() {
        let past = Utc::now() - TimeDelta::seconds(60);
        assert!(past.time_from_now() < TimeDelta::zero());
    }
}
