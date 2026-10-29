//! Positions and lengths in a video, stored as whole microseconds.
//!
//! Microseconds are ffmpeg's own internal time base (`AV_TIME_BASE`), so
//! values round-trip through ffmpeg arguments and progress reports exactly.

use std::fmt;

const MICROS_PER_MILLI: u64 = 1_000;
const MICROS_PER_SECOND: u64 = 1_000_000;
const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = 3_600;

/// A non-negative point in time (or length of time) within a video.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Timestamp(u64);

impl Timestamp {
    /// The start of the video.
    pub const ZERO: Self = Self(0);

    /// Creates a timestamp from whole microseconds.
    #[must_use]
    pub const fn from_micros(micros: u64) -> Self {
        Self(micros)
    }

    /// Creates a timestamp from whole milliseconds.
    #[must_use]
    pub const fn from_millis(millis: u64) -> Self {
        Self(millis.saturating_mul(MICROS_PER_MILLI))
    }

    /// Creates a timestamp from whole seconds.
    #[must_use]
    pub const fn from_secs(secs: u64) -> Self {
        Self(secs.saturating_mul(MICROS_PER_SECOND))
    }

    /// Converts fractional seconds, rounding to the nearest microsecond.
    ///
    /// Returns `None` for negative, NaN, or infinite input.
    #[must_use]
    pub fn from_secs_f64(secs: f64) -> Option<Self> {
        if !secs.is_finite() || secs < 0.0 {
            return None;
        }
        // The value is finite, non-negative, and rounded; `as` saturates for
        // values beyond u64::MAX, which is far past any real video.
        let micros = (secs * 1_000_000.0).round() as u64;
        Some(Self(micros))
    }

    /// Whole microseconds since the start.
    #[must_use]
    pub const fn as_micros(self) -> u64 {
        self.0
    }

    /// The time `delta_micros` away from this one, clamped to `0..=max`.
    #[must_use]
    pub const fn offset_clamped(self, delta_micros: i64, max: Self) -> Self {
        let moved = self.0.saturating_add_signed(delta_micros);
        if moved > max.0 { max } else { Self(moved) }
    }

    /// `self + other`, saturating at the maximum value.
    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self(self.0.saturating_add(other.0))
    }

    /// `self - other`, or zero if `other` is later.
    #[must_use]
    pub const fn saturating_sub(self, other: Self) -> Self {
        Self(self.0.saturating_sub(other.0))
    }

    /// Seconds with microsecond precision, as ffmpeg's `-ss` and `-t` accept
    /// (for example `83.500000`). Never uses exponent notation.
    #[must_use]
    pub fn to_ffmpeg_seconds(self) -> String {
        format!(
            "{}.{:06}",
            self.0 / MICROS_PER_SECOND,
            self.0 % MICROS_PER_SECOND
        )
    }

    /// `HH-MM-SS.mmm`, safe to use in file names on every platform.
    #[must_use]
    pub fn to_file_name_part(self) -> String {
        let parts = Parts::of(self);
        format!(
            "{:02}-{:02}-{:02}.{:03}",
            parts.hours, parts.minutes, parts.seconds, parts.millis
        )
    }
}

/// Formats as `HH:MM:SS.mmm` (milliseconds truncated).
impl fmt::Display for Timestamp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let parts = Parts::of(*self);
        write!(
            f,
            "{:02}:{:02}:{:02}.{:03}",
            parts.hours, parts.minutes, parts.seconds, parts.millis
        )
    }
}

struct Parts {
    hours: u64,
    minutes: u64,
    seconds: u64,
    millis: u64,
}

impl Parts {
    const fn of(timestamp: Timestamp) -> Self {
        let total_seconds = timestamp.0 / MICROS_PER_SECOND;
        Self {
            hours: total_seconds / SECONDS_PER_HOUR,
            minutes: (total_seconds % SECONDS_PER_HOUR) / SECONDS_PER_MINUTE,
            seconds: total_seconds % SECONDS_PER_MINUTE,
            millis: (timestamp.0 % MICROS_PER_SECOND) / MICROS_PER_MILLI,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_uses_hours_minutes_seconds_and_truncated_millis() {
        assert_eq!(Timestamp::ZERO.to_string(), "00:00:00.000");
        assert_eq!(
            Timestamp::from_micros(83_456_999).to_string(),
            "00:01:23.456"
        );
        assert_eq!(
            Timestamp::from_secs(3 * 3_600 + 5).to_string(),
            "03:00:05.000"
        );
    }

    #[test]
    fn display_keeps_counting_hours_past_a_day() {
        assert_eq!(
            Timestamp::from_secs(100 * 3_600).to_string(),
            "100:00:00.000"
        );
    }

    #[test]
    fn ffmpeg_seconds_keep_full_microsecond_precision() {
        assert_eq!(Timestamp::ZERO.to_ffmpeg_seconds(), "0.000000");
        assert_eq!(
            Timestamp::from_micros(83_500_001).to_ffmpeg_seconds(),
            "83.500001"
        );
        assert_eq!(Timestamp::from_micros(7).to_ffmpeg_seconds(), "0.000007");
    }

    #[test]
    fn file_name_part_has_no_colons() {
        assert_eq!(
            Timestamp::from_micros(3_723_045_000).to_file_name_part(),
            "01-02-03.045"
        );
    }

    #[test]
    fn from_secs_f64_rounds_to_nearest_microsecond() {
        assert_eq!(
            Timestamp::from_secs_f64(12.345_678_6),
            Some(Timestamp::from_micros(12_345_679))
        );
        assert_eq!(Timestamp::from_secs_f64(0.0), Some(Timestamp::ZERO));
    }

    #[test]
    fn from_secs_f64_rejects_negative_and_non_finite_values() {
        assert_eq!(Timestamp::from_secs_f64(-0.5), None);
        assert_eq!(Timestamp::from_secs_f64(f64::NAN), None);
        assert_eq!(Timestamp::from_secs_f64(f64::INFINITY), None);
    }

    #[test]
    fn offset_clamped_stays_within_zero_and_max() {
        let max = Timestamp::from_secs(10);
        let start = Timestamp::from_secs(5);
        assert_eq!(
            start.offset_clamped(2_000_000, max),
            Timestamp::from_secs(7)
        );
        assert_eq!(start.offset_clamped(-9_000_000, max), Timestamp::ZERO);
        assert_eq!(start.offset_clamped(60_000_000, max), max);
        assert_eq!(start.offset_clamped(i64::MIN, max), Timestamp::ZERO);
    }

    #[test]
    fn saturating_arithmetic_never_wraps() {
        let one = Timestamp::from_secs(1);
        assert_eq!(Timestamp::ZERO.saturating_sub(one), Timestamp::ZERO);
        assert_eq!(
            Timestamp::from_micros(u64::MAX).saturating_add(one),
            Timestamp::from_micros(u64::MAX)
        );
    }

    #[test]
    fn constructors_agree_on_units() {
        assert_eq!(
            Timestamp::from_millis(1_500),
            Timestamp::from_micros(1_500_000)
        );
        assert_eq!(Timestamp::from_secs(2), Timestamp::from_millis(2_000));
    }
}
