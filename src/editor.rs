//! The editing session: playhead, playback state, in point, and marked clips.
//!
//! This is the core business logic. It does no I/O and reads no clock: the
//! caller passes elapsed time to [`Editor::advance`], which keeps it testable.

use crate::clip::{Clip, ClipError};
use crate::media::MediaInfo;
use crate::timecode::Timestamp;

/// Why marking an out point did not create a clip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MarkError {
    /// No in point has been set.
    #[error("set an in point first (press i)")]
    NoInPoint,
    /// The out point is not after the in point.
    #[error(transparent)]
    InvalidRange(#[from] ClipError),
    /// A clip with exactly this range already exists.
    #[error("a clip from {start} to {end} already exists")]
    Duplicate {
        /// Start of the existing clip.
        start: Timestamp,
        /// End of the existing clip.
        end: Timestamp,
    },
}

/// State of one editing session on one video.
#[derive(Debug, Clone)]
pub struct Editor {
    duration: Timestamp,
    frame_duration: Timestamp,
    playhead: Timestamp,
    is_playing: bool,
    pending_in: Option<Timestamp>,
    /// Kept sorted by start, then end, so the list reads like the timeline.
    clips: Vec<Clip>,
    selected: Option<usize>,
}

impl Editor {
    /// Starts a session at the beginning of the video, paused, with no clips.
    #[must_use]
    pub fn new(media: &MediaInfo) -> Self {
        Self {
            duration: media.duration,
            frame_duration: media.video.frame_rate.frame_duration(),
            playhead: Timestamp::ZERO,
            is_playing: false,
            pending_in: None,
            clips: Vec::new(),
            selected: None,
        }
    }

    /// Length of the video.
    #[must_use]
    pub const fn duration(&self) -> Timestamp {
        self.duration
    }

    /// Current position.
    #[must_use]
    pub const fn playhead(&self) -> Timestamp {
        self.playhead
    }

    /// Whether playback is running.
    #[must_use]
    pub const fn is_playing(&self) -> bool {
        self.is_playing
    }

    /// The in point waiting for an out point, if any.
    #[must_use]
    pub const fn pending_in(&self) -> Option<Timestamp> {
        self.pending_in
    }

    /// All marked clips, sorted by start time.
    #[must_use]
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// Index of the selected clip in [`Editor::clips`].
    #[must_use]
    pub const fn selected_index(&self) -> Option<usize> {
        self.selected
    }

    /// The position to show in the preview.
    ///
    /// At the very end of the file there is no frame to decode, so this backs
    /// off by one frame from the end.
    #[must_use]
    pub fn preview_position(&self) -> Timestamp {
        let last_frame = self.duration.saturating_sub(self.frame_duration);
        self.playhead.min(last_frame)
    }

    /// Moves the playhead by `delta_micros` (negative moves back), clamped to
    /// the video. Playback continues if it was running.
    pub const fn seek_by(&mut self, delta_micros: i64) {
        self.playhead = self.playhead.offset_clamped(delta_micros, self.duration);
    }

    /// Moves the playhead by whole frames and pauses, for precise positioning.
    pub fn step_frames(&mut self, frames: i64) {
        let frame_micros = i64::try_from(self.frame_duration.as_micros()).unwrap_or(i64::MAX);
        self.is_playing = false;
        self.seek_by(frames.saturating_mul(frame_micros));
    }

    /// Moves the playhead to `at`, clamped to the video.
    pub fn seek_to(&mut self, at: Timestamp) {
        self.playhead = at.min(self.duration);
    }

    /// Starts or pauses playback. Starting at the end restarts from the beginning.
    pub fn toggle_playback(&mut self) {
        if !self.is_playing && self.playhead >= self.duration {
            self.playhead = Timestamp::ZERO;
        }
        self.is_playing = !self.is_playing;
    }

    /// Pauses playback.
    pub const fn pause(&mut self) {
        self.is_playing = false;
    }

    /// Advances the playhead by `elapsed` if playing; stops at the end.
    pub fn advance(&mut self, elapsed: Timestamp) {
        if !self.is_playing {
            return;
        }
        self.playhead = self.playhead.saturating_add(elapsed).min(self.duration);
        if self.playhead >= self.duration {
            self.is_playing = false;
        }
    }

    /// Sets the in point at the playhead, replacing any earlier in point.
    pub const fn mark_in(&mut self) {
        self.pending_in = Some(self.playhead);
    }

    /// Discards the pending in point. Returns whether there was one.
    pub const fn clear_in(&mut self) -> bool {
        self.pending_in.take().is_some()
    }

    /// Creates a clip from the pending in point to the playhead and selects it.
    ///
    /// On error the in point is kept so the user can move and try again.
    ///
    /// # Errors
    ///
    /// Returns [`MarkError`] if there is no in point, the range is empty or
    /// backwards, or the same clip already exists.
    pub fn mark_out(&mut self) -> Result<Clip, MarkError> {
        let start = self.pending_in.ok_or(MarkError::NoInPoint)?;
        let clip = Clip::new(start, self.playhead)?;
        let index = self
            .clips
            .binary_search(&clip)
            .err()
            .ok_or(MarkError::Duplicate {
                start: clip.start(),
                end: clip.end(),
            })?;
        self.clips.insert(index, clip);
        self.selected = Some(index);
        self.pending_in = None;
        Ok(clip)
    }

    /// Selects the next clip (or the first if none is selected).
    pub fn select_next(&mut self) {
        self.selected = match (self.selected, self.clips.len()) {
            (_, 0) => None,
            (None, _) => Some(0),
            (Some(index), len) => Some((index + 1).min(len - 1)),
        };
    }

    /// Selects the previous clip (or the last if none is selected).
    pub fn select_previous(&mut self) {
        self.selected = match (self.selected, self.clips.len()) {
            (_, 0) => None,
            (None, len) => Some(len - 1),
            (Some(index), _) => Some(index.saturating_sub(1)),
        };
    }

    /// Moves the playhead to the start of the selected clip and pauses.
    pub fn jump_to_selected(&mut self) {
        if let Some(clip) = self.selected.and_then(|index| self.clips.get(index)) {
            self.playhead = clip.start();
            self.is_playing = false;
        }
    }

    /// Removes the selected clip and selects its neighbour. Returns the
    /// removed clip, or `None` if nothing was selected.
    pub fn delete_selected(&mut self) -> Option<Clip> {
        let index = self.selected?;
        if index >= self.clips.len() {
            self.selected = None;
            return None;
        }
        let removed = self.clips.remove(index);
        self.selected = if self.clips.is_empty() {
            None
        } else {
            Some(index.min(self.clips.len() - 1))
        };
        Some(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::{FrameRate, VideoStream};

    fn secs(value: u64) -> Timestamp {
        Timestamp::from_secs(value)
    }

    /// A 60 s, 25 fps video: one frame is 40 ms.
    fn editor() -> Editor {
        Editor::new(&MediaInfo {
            duration: secs(60),
            video: VideoStream {
                width: 1920,
                height: 1080,
                frame_rate: FrameRate::new(25, 1).unwrap(),
            },
            has_audio: true,
        })
    }

    fn editor_with_clips(ranges: &[(u64, u64)]) -> Editor {
        let mut editor = editor();
        for &(start, end) in ranges {
            editor.seek_to(secs(start));
            editor.mark_in();
            editor.seek_to(secs(end));
            editor.mark_out().unwrap();
        }
        editor
    }

    #[test]
    fn new_session_starts_paused_at_zero_with_nothing_marked() {
        let editor = editor();
        assert_eq!(editor.playhead(), Timestamp::ZERO);
        assert!(!editor.is_playing());
        assert_eq!(editor.pending_in(), None);
        assert!(editor.clips().is_empty());
        assert_eq!(editor.selected_index(), None);
    }

    #[test]
    fn seek_by_clamps_to_the_video() {
        let mut editor = editor();
        editor.seek_by(-1_000_000);
        assert_eq!(editor.playhead(), Timestamp::ZERO);
        editor.seek_by(90_000_000);
        assert_eq!(editor.playhead(), secs(60));
    }

    #[test]
    fn step_frames_moves_by_frame_duration_and_pauses() {
        let mut editor = editor();
        editor.toggle_playback();
        editor.step_frames(3);
        assert_eq!(editor.playhead(), Timestamp::from_millis(120));
        assert!(!editor.is_playing());
        editor.step_frames(-1);
        assert_eq!(editor.playhead(), Timestamp::from_millis(80));
    }

    #[test]
    fn seek_to_clamps_past_the_end() {
        let mut editor = editor();
        editor.seek_to(secs(500));
        assert_eq!(editor.playhead(), secs(60));
    }

    #[test]
    fn advance_moves_only_while_playing() {
        let mut editor = editor();
        editor.advance(secs(1));
        assert_eq!(editor.playhead(), Timestamp::ZERO);
        editor.toggle_playback();
        editor.advance(Timestamp::from_millis(500));
        assert_eq!(editor.playhead(), Timestamp::from_millis(500));
    }

    #[test]
    fn playback_stops_at_the_end() {
        let mut editor = editor();
        editor.seek_to(secs(59));
        editor.toggle_playback();
        editor.advance(secs(5));
        assert_eq!(editor.playhead(), secs(60));
        assert!(!editor.is_playing());
    }

    #[test]
    fn starting_playback_at_the_end_restarts_from_zero() {
        let mut editor = editor();
        editor.seek_to(secs(60));
        editor.toggle_playback();
        assert!(editor.is_playing());
        assert_eq!(editor.playhead(), Timestamp::ZERO);
    }

    #[test]
    fn preview_position_backs_off_one_frame_at_the_end() {
        let mut editor = editor();
        editor.seek_to(secs(60));
        assert_eq!(editor.preview_position(), Timestamp::from_millis(59_960));
        editor.seek_to(secs(10));
        assert_eq!(editor.preview_position(), secs(10));
    }

    #[test]
    fn mark_out_creates_clip_from_in_point_and_selects_it() {
        let mut editor = editor();
        editor.seek_to(secs(5));
        editor.mark_in();
        editor.seek_to(secs(8));
        let clip = editor.mark_out().unwrap();
        assert_eq!(clip, Clip::new(secs(5), secs(8)).unwrap());
        assert_eq!(editor.clips(), &[clip]);
        assert_eq!(editor.selected_index(), Some(0));
        assert_eq!(editor.pending_in(), None);
    }

    #[test]
    fn mark_out_without_in_point_fails() {
        let mut editor = editor();
        assert_eq!(editor.mark_out(), Err(MarkError::NoInPoint));
    }

    #[test]
    fn mark_out_before_in_point_fails_and_keeps_the_in_point() {
        let mut editor = editor();
        editor.seek_to(secs(10));
        editor.mark_in();
        editor.seek_to(secs(4));
        assert!(matches!(editor.mark_out(), Err(MarkError::InvalidRange(_))));
        assert_eq!(editor.pending_in(), Some(secs(10)));
        assert!(editor.clips().is_empty());
    }

    #[test]
    fn mark_out_at_in_point_fails() {
        let mut editor = editor();
        editor.mark_in();
        assert!(matches!(editor.mark_out(), Err(MarkError::InvalidRange(_))));
    }

    #[test]
    fn duplicate_clip_is_rejected() {
        let mut editor = editor_with_clips(&[(1, 2)]);
        editor.seek_to(secs(1));
        editor.mark_in();
        editor.seek_to(secs(2));
        assert_eq!(
            editor.mark_out(),
            Err(MarkError::Duplicate {
                start: secs(1),
                end: secs(2)
            })
        );
        assert_eq!(editor.clips().len(), 1);
    }

    #[test]
    fn overlapping_clips_are_allowed_and_kept_sorted() {
        let editor = editor_with_clips(&[(20, 30), (5, 10), (8, 25)]);
        let starts: Vec<_> = editor.clips().iter().map(|clip| clip.start()).collect();
        assert_eq!(starts, vec![secs(5), secs(8), secs(20)]);
        assert_eq!(editor.selected_index(), Some(1), "newest clip is selected");
    }

    #[test]
    fn mark_in_replaces_the_previous_in_point_and_clear_in_discards_it() {
        let mut editor = editor();
        editor.seek_to(secs(3));
        editor.mark_in();
        editor.seek_to(secs(7));
        editor.mark_in();
        assert_eq!(editor.pending_in(), Some(secs(7)));
        assert!(editor.clear_in());
        assert!(!editor.clear_in());
        assert_eq!(editor.pending_in(), None);
    }

    #[test]
    fn selection_moves_within_bounds() {
        let mut editor = editor_with_clips(&[(1, 2), (3, 4), (5, 6)]);
        editor.select_next();
        assert_eq!(editor.selected_index(), Some(2), "stays on the last clip");
        editor.select_previous();
        editor.select_previous();
        editor.select_previous();
        assert_eq!(editor.selected_index(), Some(0), "stays on the first clip");
    }

    #[test]
    fn selection_with_no_clips_stays_empty() {
        let mut editor = editor();
        editor.select_next();
        editor.select_previous();
        assert_eq!(editor.selected_index(), None);
    }

    #[test]
    fn jump_to_selected_moves_playhead_to_clip_start_and_pauses() {
        let mut editor = editor_with_clips(&[(12, 15)]);
        editor.toggle_playback();
        editor.jump_to_selected();
        assert_eq!(editor.playhead(), secs(12));
        assert!(!editor.is_playing());
    }

    #[test]
    fn delete_selected_removes_clip_and_selects_neighbour() {
        let mut editor = editor_with_clips(&[(1, 2), (3, 4), (5, 6)]);
        editor.select_previous();
        assert_eq!(editor.selected_index(), Some(1));
        let removed = editor.delete_selected().unwrap();
        assert_eq!(removed.start(), secs(3));
        assert_eq!(editor.selected_index(), Some(1));
        editor.delete_selected();
        assert_eq!(
            editor.selected_index(),
            Some(0),
            "moves to the new last clip"
        );
        editor.delete_selected();
        assert_eq!(editor.selected_index(), None);
        assert_eq!(editor.delete_selected(), None);
    }
}
