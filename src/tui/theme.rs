//! Colour themes: every colour in the UI comes from one [`Theme`]
//! (see `docs/adr/0003-design-system-and-themes.md`).

use ratatui::style::{Color, Modifier, Style};

use crate::cli::ThemeChoice;

/// Named colours, with the token names OpenCode's themes use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Theme {
    pub(crate) primary: Color,
    pub(crate) secondary: Color,
    pub(crate) accent: Color,
    pub(crate) text: Color,
    pub(crate) text_muted: Color,
    pub(crate) background: Color,
    pub(crate) background_panel: Color,
    pub(crate) background_element: Color,
    pub(crate) border: Color,
    pub(crate) border_active: Color,
    pub(crate) border_subtle: Color,
    pub(crate) success: Color,
    pub(crate) warning: Color,
    pub(crate) error: Color,
    pub(crate) info: Color,
}

impl Theme {
    /// 24-bit dark theme, approximating the look of OpenCode's default.
    pub(crate) const DARK: Self = Self {
        primary: Color::Rgb(0x5c, 0x9c, 0xf5),
        secondary: Color::Rgb(0x9d, 0x7c, 0xd8),
        accent: Color::Rgb(0xd8, 0x7c, 0xc8),
        text: Color::Rgb(0xee, 0xee, 0xee),
        text_muted: Color::Rgb(0x80, 0x80, 0x80),
        background: Color::Rgb(0x0a, 0x0a, 0x0a),
        background_panel: Color::Rgb(0x14, 0x14, 0x14),
        background_element: Color::Rgb(0x1e, 0x1e, 0x1e),
        border: Color::Rgb(0x48, 0x48, 0x48),
        border_active: Color::Rgb(0x60, 0x60, 0x60),
        border_subtle: Color::Rgb(0x32, 0x32, 0x32),
        success: Color::Rgb(0x7f, 0xd8, 0x8f),
        warning: Color::Rgb(0xf5, 0xa7, 0x42),
        error: Color::Rgb(0xe0, 0x6c, 0x75),
        info: Color::Rgb(0x56, 0xb6, 0xc2),
    };

    /// The terminal's own ANSI colours and default background.
    pub(crate) const SYSTEM: Self = Self {
        primary: Color::Blue,
        secondary: Color::Magenta,
        accent: Color::Magenta,
        text: Color::Reset,
        text_muted: Color::DarkGray,
        background: Color::Reset,
        background_panel: Color::Reset,
        background_element: Color::DarkGray,
        border: Color::DarkGray,
        border_active: Color::Gray,
        border_subtle: Color::DarkGray,
        success: Color::Green,
        warning: Color::Yellow,
        error: Color::Red,
        info: Color::Cyan,
    };

    /// The theme for `choice`; `auto` looks at what the terminal reports.
    pub(crate) fn resolve(choice: ThemeChoice, terminal: &TerminalInfo) -> Self {
        match choice {
            ThemeChoice::Dark => Self::DARK,
            ThemeChoice::System => Self::SYSTEM,
            ThemeChoice::Auto if terminal.supports_truecolor() => Self::DARK,
            ThemeChoice::Auto => Self::SYSTEM,
        }
    }

    /// The screen background (and default text colour).
    pub(crate) fn base(&self) -> Style {
        Style::new().bg(self.background).fg(self.text)
    }

    /// Secondary text: hints, empty states, file names in progress.
    pub(crate) fn muted(&self) -> Style {
        Style::new().fg(self.text_muted)
    }

    /// A small label with a filled background, like the `clipedit` title.
    pub(crate) fn chip(&self) -> Style {
        Style::new()
            .bg(self.primary)
            .fg(self.background_or_black())
            .add_modifier(Modifier::BOLD)
    }

    /// Text on a `primary` chip must contrast even when the background is
    /// the terminal's default.
    const fn background_or_black(&self) -> Color {
        match self.background {
            Color::Reset => Color::Black,
            color => color,
        }
    }
}

/// What the terminal says about itself through environment variables.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct TerminalInfo {
    /// `COLORTERM`.
    pub(crate) colorterm: Option<String>,
    /// Whether `WT_SESSION` is set (Windows Terminal).
    pub(crate) windows_terminal: bool,
    /// `TERM_PROGRAM`.
    pub(crate) term_program: Option<String>,
}

/// `TERM_PROGRAM` values of terminals known to support 24-bit colour.
const TRUECOLOR_PROGRAMS: &[&str] = &["iTerm", "WezTerm", "vscode"];

impl TerminalInfo {
    /// Reads the variables from the environment.
    pub(crate) fn from_env() -> Self {
        Self {
            colorterm: std::env::var("COLORTERM").ok(),
            windows_terminal: std::env::var_os("WT_SESSION").is_some(),
            term_program: std::env::var("TERM_PROGRAM").ok(),
        }
    }

    fn supports_truecolor(&self) -> bool {
        let colorterm = self.colorterm.as_deref().is_some_and(|value| {
            value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
        });
        let known_program = self
            .term_program
            .as_deref()
            .is_some_and(|name| TRUECOLOR_PROGRAMS.iter().any(|known| name.contains(known)));
        colorterm || self.windows_terminal || known_program
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(terminal: TerminalInfo) -> Theme {
        Theme::resolve(ThemeChoice::Auto, &terminal)
    }

    #[test]
    fn auto_uses_dark_when_the_terminal_reports_truecolor() {
        let colorterm = TerminalInfo {
            colorterm: Some("truecolor".to_owned()),
            ..TerminalInfo::default()
        };
        assert_eq!(auto(colorterm), Theme::DARK);
        let wt = TerminalInfo {
            windows_terminal: true,
            ..TerminalInfo::default()
        };
        assert_eq!(auto(wt), Theme::DARK);
        let wezterm = TerminalInfo {
            term_program: Some("WezTerm".to_owned()),
            ..TerminalInfo::default()
        };
        assert_eq!(auto(wezterm), Theme::DARK);
    }

    #[test]
    fn auto_falls_back_to_system_colours() {
        assert_eq!(auto(TerminalInfo::default()), Theme::SYSTEM);
        let colorterm_256 = TerminalInfo {
            colorterm: Some("256color".to_owned()),
            ..TerminalInfo::default()
        };
        assert_eq!(auto(colorterm_256), Theme::SYSTEM);
    }

    #[test]
    fn explicit_choices_ignore_the_terminal() {
        let truecolor = TerminalInfo {
            colorterm: Some("24bit".to_owned()),
            ..TerminalInfo::default()
        };
        assert_eq!(
            Theme::resolve(ThemeChoice::System, &truecolor),
            Theme::SYSTEM
        );
        assert_eq!(
            Theme::resolve(ThemeChoice::Dark, &TerminalInfo::default()),
            Theme::DARK
        );
    }

    #[test]
    fn chip_text_stays_readable_on_the_system_theme() {
        assert_eq!(Theme::SYSTEM.chip().fg, Some(Color::Black));
        assert_eq!(Theme::DARK.chip().fg, Some(Theme::DARK.background));
    }
}
