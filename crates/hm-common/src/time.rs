//! System-clock, epoch, and now-relative time helpers.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use chrono::{DateTime, TimeDelta, Utc};
use num_traits::Bounded;

mod sealed {
    pub trait SealedDuration {}
    impl SealedDuration for std::time::Duration {}

    pub trait SealedDateTime {}
    impl SealedDateTime for chrono::DateTime<chrono::Utc> {}
}

/// Extension trait adding system-clock readings to [`Duration`].
pub trait DurationExt: sealed::SealedDuration {
    /// The current Unix time in whole seconds, as the requested integer type.
    ///
    /// Reads the system clock via [`SystemTime::now`], returning `0` if the
    /// clock is set before the Unix epoch. If the `u64` second count does not
    /// fit `T` (e.g. it exceeds `i64::MAX`, unreachable for ~292 billion years),
    /// this logs via [`tracing::error!`] and saturates at `T::max_value()`
    /// rather than wrapping or panicking.
    ///
    /// ```
    /// # use hm_common::time::DurationExt;
    /// # use std::time::Duration;
    /// let now = Duration::now_epoch_secs_saturating::<i64>();
    /// assert!(now > 0);
    /// ```
    #[must_use]
    fn now_epoch_secs_saturating<T: TryFrom<u64> + Bounded>() -> T;
}

impl DurationExt for Duration {
    fn now_epoch_secs_saturating<T: TryFrom<u64> + Bounded>() -> T {
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        T::try_from(secs).unwrap_or_else(|_| {
            tracing::error!(
                secs,
                ty = std::any::type_name::<T>(),
                "Unix epoch seconds do not fit target integer; saturating at its maximum"
            );
            T::max_value()
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
        let now = Duration::now_epoch_secs_saturating::<i64>();
        // 1_577_836_800 = 2020-01-01, 4_102_444_800 = 2100-01-01.
        assert!(
            now > 1_577_836_800,
            "epoch secs {now} implausibly small (< 2020)"
        );
        assert!(
            now < 4_102_444_800,
            "epoch secs {now} implausibly large (> 2100)"
        );
    }

    #[rstest]
    fn successive_reads_are_nondecreasing() {
        let a = Duration::now_epoch_secs_saturating::<i64>();
        let b = Duration::now_epoch_secs_saturating::<i64>();
        assert!(b >= a, "clock went backwards: {a} then {b}");
    }

    #[rstest]
    fn saturates_at_target_max_when_seconds_do_not_fit() {
        // Current epoch seconds (~1.7e9) far exceed the range of a narrow type,
        // so this deterministically exercises the saturating-overflow branch
        // that the `i64` version could never reach.
        assert_eq!(Duration::now_epoch_secs_saturating::<i8>(), i8::MAX);
        assert_eq!(Duration::now_epoch_secs_saturating::<u8>(), u8::MAX);
        assert_eq!(Duration::now_epoch_secs_saturating::<u16>(), u16::MAX);
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
