//! A marked range of a video that will be exported as its own file.

use crate::timecode::Timestamp;

/// A non-empty range `[start, end)` of a video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Clip {
    start: Timestamp,
    end: Timestamp,
}

/// Why a range cannot be a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ClipError {
    /// The end is at or before the start.
    #[error("the out point ({end}) must be after the in point ({start})")]
    EndNotAfterStart {
        /// Requested start.
        start: Timestamp,
        /// Requested end.
        end: Timestamp,
    },
}

impl Clip {
    /// Creates a clip from `start` (inclusive) to `end` (exclusive).
    ///
    /// # Errors
    ///
    /// Returns [`ClipError::EndNotAfterStart`] if `end <= start`.
    pub fn new(start: Timestamp, end: Timestamp) -> Result<Self, ClipError> {
        if end <= start {
            return Err(ClipError::EndNotAfterStart { start, end });
        }
        Ok(Self { start, end })
    }

    /// Where the clip begins.
    #[must_use]
    pub const fn start(self) -> Timestamp {
        self.start
    }

    /// Where the clip ends.
    #[must_use]
    pub const fn end(self) -> Timestamp {
        self.end
    }

    /// The clip's length (always greater than zero).
    #[must_use]
    pub const fn length(self) -> Timestamp {
        self.end.saturating_sub(self.start)
    }

    /// Whether `at` lies inside the clip.
    #[must_use]
    pub fn contains(self, at: Timestamp) -> bool {
        self.start <= at && at < self.end
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(value: u64) -> Timestamp {
        Timestamp::from_secs(value)
    }

    #[test]
    fn new_accepts_a_forward_range() {
        let clip = Clip::new(secs(2), secs(5)).unwrap();
        assert_eq!(clip.start(), secs(2));
        assert_eq!(clip.end(), secs(5));
        assert_eq!(clip.length(), secs(3));
    }

    #[test]
    fn new_rejects_empty_and_backward_ranges() {
        assert_eq!(
            Clip::new(secs(3), secs(3)),
            Err(ClipError::EndNotAfterStart {
                start: secs(3),
                end: secs(3)
            })
        );
        assert!(Clip::new(secs(5), secs(2)).is_err());
    }

    #[test]
    fn contains_includes_start_and_excludes_end() {
        let clip = Clip::new(secs(2), secs(5)).unwrap();
        assert!(clip.contains(secs(2)));
        assert!(clip.contains(Timestamp::from_micros(4_999_999)));
        assert!(!clip.contains(secs(5)));
        assert!(!clip.contains(Timestamp::from_micros(1_999_999)));
    }

    #[test]
    fn error_message_names_both_points() {
        let error = Clip::new(secs(5), secs(2)).unwrap_err();
        assert_eq!(
            error.to_string(),
            "the out point (00:00:02.000) must be after the in point (00:00:05.000)"
        );
    }
}
