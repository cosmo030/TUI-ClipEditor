//! A one-line timeline: the whole video, its clips, the in point, and the playhead.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::Widget;

use crate::clip::Clip;
use crate::timecode::Timestamp;

const EMPTY: &str = "─";
const CLIP: &str = "█";
const PENDING: &str = "▒";
const IN_POINT: &str = "[";
const PLAYHEAD: &str = "┃";

/// Draws the video's timeline into a single row.
#[derive(Debug)]
pub(super) struct Timeline<'a> {
    pub(super) duration: Timestamp,
    pub(super) playhead: Timestamp,
    pub(super) pending_in: Option<Timestamp>,
    pub(super) clips: &'a [Clip],
    pub(super) selected: Option<usize>,
}

impl Timeline<'_> {
    /// The column (0-based, within `width`) that shows `at`.
    fn column(&self, at: Timestamp, width: u16) -> u16 {
        let duration = u128::from(self.duration.as_micros().max(1));
        let column = u128::from(at.as_micros()) * u128::from(width) / duration;
        u16::try_from(column)
            .unwrap_or(u16::MAX)
            .min(width.saturating_sub(1))
    }

    /// The time range `[start, end)` that column `column` covers.
    fn column_range(&self, column: u16, width: u16) -> (Timestamp, Timestamp) {
        let duration = u128::from(self.duration.as_micros());
        let at = |column: u16| {
            let micros = duration * u128::from(column) / u128::from(width.max(1));
            Timestamp::from_micros(u64::try_from(micros).unwrap_or(u64::MAX))
        };
        (at(column), at(column + 1))
    }

    fn cell(&self, column: u16, width: u16) -> (&'static str, Style) {
        if column == self.column(self.playhead, width) {
            return (
                PLAYHEAD,
                Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
            );
        }
        if self
            .pending_in
            .is_some_and(|at| column == self.column(at, width))
        {
            return (
                IN_POINT,
                Style::new().fg(Color::Magenta).add_modifier(Modifier::BOLD),
            );
        }
        let (start, end) = self.column_range(column, width);
        let overlaps = |clip: &Clip| clip.start() < end && clip.end() > start;
        if self.clips.iter().any(overlaps) {
            let is_selected = self
                .selected
                .and_then(|selected| self.clips.get(selected))
                .is_some_and(overlaps);
            let color = if is_selected {
                Color::Yellow
            } else {
                Color::Green
            };
            return (CLIP, Style::new().fg(color));
        }
        if let Some(in_point) = self.pending_in {
            let (from, to) = if in_point <= self.playhead {
                (in_point, self.playhead)
            } else {
                (self.playhead, in_point)
            };
            if from < end && to > start {
                return (PENDING, Style::new().fg(Color::Magenta));
            }
        }
        (EMPTY, Style::new().fg(Color::DarkGray))
    }
}

impl Widget for Timeline<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        for column in 0..area.width {
            let (symbol, style) = self.cell(column, area.width);
            buf[(area.x + column, area.y)]
                .set_symbol(symbol)
                .set_style(style);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn secs(value: u64) -> Timestamp {
        Timestamp::from_secs(value)
    }

    /// Renders a 10 s timeline into 10 columns (one second per column).
    fn render(timeline: Timeline<'_>) -> (String, Buffer) {
        let area = Rect::new(0, 0, 10, 1);
        let mut buffer = Buffer::empty(area);
        timeline.render(area, &mut buffer);
        let symbols = (0..10)
            .map(|x| buffer[(x, 0)].symbol().to_owned())
            .collect();
        (symbols, buffer)
    }

    fn timeline(clips: &[Clip]) -> Timeline<'_> {
        Timeline {
            duration: secs(10),
            playhead: Timestamp::ZERO,
            pending_in: None,
            clips,
            selected: None,
        }
    }

    #[test]
    fn empty_timeline_shows_only_the_playhead() {
        let (symbols, _) = render(timeline(&[]));
        assert_eq!(symbols, "┃─────────");
    }

    #[test]
    fn playhead_at_the_end_stays_inside_the_bar() {
        let (symbols, _) = render(Timeline {
            playhead: secs(10),
            ..timeline(&[])
        });
        assert_eq!(symbols, "─────────┃");
    }

    #[test]
    fn clips_fill_the_columns_they_cover() {
        let clips = [Clip::new(secs(2), secs(5)).unwrap()];
        let (symbols, _) = render(timeline(&clips));
        assert_eq!(symbols, "┃─███─────");
    }

    #[test]
    fn selected_clip_is_highlighted() {
        let clips = [
            Clip::new(secs(1), secs(2)).unwrap(),
            Clip::new(secs(6), secs(8)).unwrap(),
        ];
        let (_, buffer) = render(Timeline {
            selected: Some(1),
            ..timeline(&clips)
        });
        assert_eq!(buffer[(1, 0)].fg, Color::Green);
        assert_eq!(buffer[(6, 0)].fg, Color::Yellow);
    }

    #[test]
    fn pending_range_runs_from_in_point_to_playhead() {
        let (symbols, _) = render(Timeline {
            pending_in: Some(secs(3)),
            playhead: secs(7),
            ..timeline(&[])
        });
        assert_eq!(symbols, "───[▒▒▒┃──");
    }

    #[test]
    fn zero_width_area_draws_nothing() {
        let area = Rect::new(0, 0, 0, 1);
        let mut buffer = Buffer::empty(Rect::new(0, 0, 1, 1));
        timeline(&[]).render(area, &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), " ");
    }
}
