//! The `clipedit` wordmark for the home screen, in block characters: "clip"
//! in muted text and "edit" in full text, like OpenCode's two-tone logo.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use super::theme::Theme;

/// "clip", three rows tall.
const CLIP: [&str; 3] = ["█▀▀ █ ▄ █▀█", "█   █ █ █▀▀", "▀▀▀ ▀ ▀ ▀  "];
/// "edit", three rows tall.
const EDIT: [&str; 3] = ["█▀█   █ ▄ ▄█▄", "█▀▀ █▀█ █  █ ", "▀▀▀ ▀▀▀ ▀  ▀▀"];
/// Space between the two words.
const GAP: &str = "  ";

/// Rows of the logo.
pub(super) const HEIGHT: u16 = 3;

/// The logo's width in cells.
pub(super) fn width() -> u16 {
    let cells = CLIP[0].chars().count() + GAP.len() + EDIT[0].chars().count();
    u16::try_from(cells).unwrap_or(u16::MAX)
}

/// The logo as styled lines.
pub(super) fn lines(theme: &Theme) -> Vec<Line<'static>> {
    let clip = Style::new().fg(theme.text_muted);
    let edit = Style::new().fg(theme.text).add_modifier(Modifier::BOLD);
    CLIP.iter()
        .zip(EDIT)
        .map(|(clip_row, edit_row)| {
            Line::from(vec![
                Span::styled(*clip_row, clip),
                Span::raw(GAP),
                Span::styled(edit_row, edit),
            ])
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_has_the_same_width() {
        for rows in [CLIP, EDIT] {
            let widths: Vec<usize> = rows.iter().map(|row| row.chars().count()).collect();
            assert!(
                widths.windows(2).all(|pair| pair[0] == pair[1]),
                "{widths:?}"
            );
        }
        assert_eq!(width(), 26);
    }

    #[test]
    fn the_two_words_use_different_tones() {
        let lines = lines(&Theme::DARK);
        assert_eq!(lines.len(), usize::from(HEIGHT));
        assert_eq!(lines[0].spans[0].style.fg, Some(Theme::DARK.text_muted));
        assert_eq!(lines[0].spans[2].style.fg, Some(Theme::DARK.text));
    }
}
