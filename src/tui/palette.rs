//! The command palette (`ctrl+p`, as in OpenCode): a searchable list of
//! every action on the current screen.

use super::fuzzy;
use super::keys::EditAction;

/// What a palette entry does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaletteAction {
    /// The same as pressing the editor key.
    Edit(EditAction),
    /// Switch between fast copy and precise export.
    ToggleExportMode,
    /// Switch between the dark and system themes.
    SwitchTheme,
    /// Home screen: go up a folder.
    UpFolder,
    /// Home screen: quit.
    Quit,
}

/// One palette entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Command {
    pub(crate) label: &'static str,
    /// The direct key, shown on the right ("" if there is none).
    pub(crate) keys: &'static str,
    pub(crate) action: PaletteAction,
}

const fn edit(label: &'static str, keys: &'static str, action: EditAction) -> Command {
    Command {
        label,
        keys,
        action: PaletteAction::Edit(action),
    }
}

/// Commands while editing.
pub(crate) const EDITOR_COMMANDS: &[Command] = &[
    edit("Play / pause", "space", EditAction::TogglePlayback),
    edit("Set in point", "i", EditAction::MarkIn),
    edit("Set out point (add clip)", "o", EditAction::MarkOut),
    edit("Clear in point", "esc", EditAction::ClearIn),
    edit("Jump to selected clip", "enter", EditAction::JumpToSelected),
    edit("Delete selected clip", "d", EditAction::DeleteSelected),
    edit("Jump to start", "home", EditAction::JumpToStart),
    edit("Jump to end", "end", EditAction::JumpToEnd),
    edit("Export clips…", "e", EditAction::OpenExport),
    Command {
        label: "Switch export mode (fast copy / precise)",
        keys: "",
        action: PaletteAction::ToggleExportMode,
    },
    edit("Open another video", "b", EditAction::Browse),
    Command {
        label: "Switch theme (dark / system)",
        keys: "",
        action: PaletteAction::SwitchTheme,
    },
    edit("Show keys", "?", EditAction::ToggleHelp),
    edit("Quit", "q", EditAction::Quit),
];

/// Commands on the home screen.
pub(crate) const HOME_COMMANDS: &[Command] = &[
    Command {
        label: "Go up a folder",
        keys: "backspace",
        action: PaletteAction::UpFolder,
    },
    Command {
        label: "Switch theme (dark / system)",
        keys: "",
        action: PaletteAction::SwitchTheme,
    },
    Command {
        label: "Quit",
        keys: "esc",
        action: PaletteAction::Quit,
    },
];

/// Palette state: the commands on offer, the search, and the selection.
#[derive(Debug, Clone)]
pub(crate) struct Palette {
    commands: &'static [Command],
    pub(crate) query: String,
    /// Index into [`Palette::matches`].
    pub(crate) selected: usize,
}

impl Palette {
    pub(crate) const fn new(commands: &'static [Command]) -> Self {
        Self {
            commands,
            query: String::new(),
            selected: 0,
        }
    }

    /// Commands matching the search, best first.
    pub(crate) fn matches(&self) -> Vec<&'static Command> {
        let commands = self.commands;
        fuzzy::ranked(commands.iter().map(|command| command.label), &self.query)
            .into_iter()
            .map(|index| &commands[index])
            .collect()
    }

    pub(crate) fn selected_command(&self) -> Option<&'static Command> {
        self.matches().get(self.selected).copied()
    }

    pub(crate) fn type_char(&mut self, character: char) {
        self.query.push(character);
        self.selected = 0;
    }

    pub(crate) fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    pub(crate) fn select_next(&mut self) {
        if self.selected + 1 < self.matches().len() {
            self.selected += 1;
        }
    }

    pub(crate) const fn select_previous(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(palette: &Palette) -> Vec<&'static str> {
        palette
            .matches()
            .iter()
            .map(|command| command.label)
            .collect()
    }

    #[test]
    fn without_a_search_every_command_is_listed_in_order() {
        let palette = Palette::new(EDITOR_COMMANDS);
        assert_eq!(labels(&palette).len(), EDITOR_COMMANDS.len());
        assert_eq!(labels(&palette)[0], "Play / pause");
    }

    #[test]
    fn typing_filters_and_ranks_commands() {
        let mut palette = Palette::new(EDITOR_COMMANDS);
        for character in "export".chars() {
            palette.type_char(character);
        }
        assert_eq!(
            labels(&palette),
            ["Export clips…", "Switch export mode (fast copy / precise)"]
        );
        assert_eq!(
            palette.selected_command().unwrap().action,
            PaletteAction::Edit(EditAction::OpenExport)
        );
    }

    #[test]
    fn selection_stays_within_the_matches_and_resets_on_typing() {
        let mut palette = Palette::new(HOME_COMMANDS);
        for _ in 0..10 {
            palette.select_next();
        }
        assert_eq!(palette.selected, HOME_COMMANDS.len() - 1);
        palette.type_char('q');
        assert_eq!(palette.selected, 0);
        palette.select_previous();
        assert_eq!(palette.selected, 0);
    }

    #[test]
    fn backspace_widens_the_search_again() {
        let mut palette = Palette::new(HOME_COMMANDS);
        palette.type_char('z');
        assert!(palette.matches().is_empty());
        palette.backspace();
        assert_eq!(palette.matches().len(), HOME_COMMANDS.len());
    }

    #[test]
    fn both_screens_can_switch_the_theme() {
        for commands in [EDITOR_COMMANDS, HOME_COMMANDS] {
            assert!(
                commands
                    .iter()
                    .any(|command| command.action == PaletteAction::SwitchTheme)
            );
        }
    }
}
