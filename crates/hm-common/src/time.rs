//! System-clock and epoch helpers.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

mod sealed {
    pub trait Sealed {}
    impl Sealed for std::time::Duration {}
}

/// Extension trait adding system-clock readings to [`Duration`].
pub trait DurationExt: sealed::Sealed {
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
}
