//! What the editor knows about an opened video.

use crate::timecode::Timestamp;

/// The properties of an opened video file that the editor needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaInfo {
    /// Total length of the file.
    pub duration: Timestamp,
    /// The video stream shown in the preview.
    pub video: VideoStream,
    /// Whether the file has at least one audio stream.
    pub has_audio: bool,
}

/// The primary video stream of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoStream {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Frames per second as a fraction (for example 30000/1001).
    pub frame_rate: FrameRate,
}

/// A frame rate expressed as `numerator / denominator` frames per second.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameRate {
    numerator: u32,
    denominator: u32,
}

impl FrameRate {
    /// Creates a frame rate; returns `None` if either part is zero.
    #[must_use]
    pub const fn new(numerator: u32, denominator: u32) -> Option<Self> {
        if numerator == 0 || denominator == 0 {
            None
        } else {
            Some(Self {
                numerator,
                denominator,
            })
        }
    }

    /// Frames per second as a decimal number, for display.
    #[must_use]
    pub fn frames_per_second(self) -> f64 {
        f64::from(self.numerator) / f64::from(self.denominator)
    }

    /// This rate, or `max_fps` frames per second if this rate is faster.
    #[must_use]
    pub fn capped(self, max_fps: u32) -> Self {
        let max_fps = max_fps.max(1);
        if u64::from(self.numerator) > u64::from(max_fps) * u64::from(self.denominator) {
            Self {
                numerator: max_fps,
                denominator: 1,
            }
        } else {
            self
        }
    }

    /// The rate as ffmpeg writes it, for example `30000/1001`.
    #[must_use]
    pub fn to_ffmpeg_rate(self) -> String {
        format!("{}/{}", self.numerator, self.denominator)
    }

    /// When frame `index` (0-based) is shown, counted from the first frame.
    /// Computed from the exact fraction, so long runs do not drift.
    #[must_use]
    pub fn frame_offset(self, index: u64) -> Timestamp {
        let numerator = u128::from(self.numerator);
        let micros = (u128::from(index) * u128::from(self.denominator) * 1_000_000 + numerator / 2)
            / numerator;
        Timestamp::from_micros(u64::try_from(micros).unwrap_or(u64::MAX))
    }

    /// The length of one frame, rounded to the nearest microsecond (at least 1).
    #[must_use]
    pub fn frame_duration(self) -> Timestamp {
        let numerator = u64::from(self.numerator);
        let micros = (u64::from(self.denominator) * 1_000_000 + numerator / 2) / numerator;
        Timestamp::from_micros(micros.max(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_parts_are_not_a_frame_rate() {
        assert_eq!(FrameRate::new(0, 1), None);
        assert_eq!(FrameRate::new(30, 0), None);
    }

    #[test]
    fn frame_duration_rounds_to_nearest_microsecond() {
        let ntsc = FrameRate::new(30_000, 1_001).unwrap();
        assert_eq!(ntsc.frame_duration(), Timestamp::from_micros(33_367));
        let pal = FrameRate::new(25, 1).unwrap();
        assert_eq!(pal.frame_duration(), Timestamp::from_micros(40_000));
    }

    #[test]
    fn frame_duration_is_never_zero() {
        let absurd = FrameRate::new(u32::MAX, 1).unwrap();
        assert_eq!(absurd.frame_duration(), Timestamp::from_micros(1));
    }

    #[test]
    fn capped_limits_only_faster_rates() {
        let sixty = FrameRate::new(60, 1).unwrap();
        assert_eq!(sixty.capped(30), FrameRate::new(30, 1).unwrap());
        let ntsc = FrameRate::new(30_000, 1_001).unwrap();
        assert_eq!(ntsc.capped(30), ntsc, "29.97 is below 30");
        assert_eq!(sixty.capped(0), FrameRate::new(1, 1).unwrap());
    }

    #[test]
    fn ffmpeg_rate_is_the_fraction() {
        assert_eq!(
            FrameRate::new(30_000, 1_001).unwrap().to_ffmpeg_rate(),
            "30000/1001"
        );
    }

    #[test]
    fn frame_offset_does_not_drift() {
        let ntsc = FrameRate::new(30_000, 1_001).unwrap();
        assert_eq!(ntsc.frame_offset(0), Timestamp::ZERO);
        // 30 000 frames at 29.97 fps last exactly 1001 s.
        assert_eq!(ntsc.frame_offset(30_000), Timestamp::from_secs(1_001));
        assert_eq!(
            FrameRate::new(25, 1).unwrap().frame_offset(3),
            Timestamp::from_millis(120)
        );
    }

    #[test]
    fn frames_per_second_is_the_fraction() {
        let ntsc = FrameRate::new(30_000, 1_001).unwrap();
        assert!((ntsc.frames_per_second() - 29.970_03).abs() < 1e-5);
    }
}
