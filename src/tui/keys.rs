//! Key bindings: translates key presses into actions for each screen.
//!
//! Keeping the mapping here, separate from what the actions do, gives one
//! place to read or change the bindings (and the help text below).

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Seek step for the arrow keys.
pub(super) const SEEK_SMALL_MICROS: i64 = 1_000_000;
/// Seek step for Shift + arrow keys.
pub(super) const SEEK_LARGE_MICROS: i64 = 10_000_000;

/// Key reference shown in the help overlay: (keys, what they do).
pub(super) const EDITOR_HELP: &[(&str, &str)] = &[
    ("Space", "play / pause"),
    ("← →", "seek 1 s"),
    ("Shift+← →", "seek 10 s"),
    (", .", "previous / next frame"),
    ("Home End", "jump to start / end"),
    ("i", "set in point"),
    ("o", "set out point (adds a clip)"),
    ("Esc", "clear in point"),
    ("↑ ↓  k j", "select clip"),
    ("Enter", "jump to selected clip"),
    ("d  Delete", "delete selected clip"),
    ("e", "export clips"),
    ("b", "browse for another video"),
    ("?", "show / hide this help"),
    ("Ctrl+P", "command palette"),
    ("q  Ctrl+C", "quit"),
];

/// Key hints for the editor's footer (the full list is in the help).
pub(super) const EDITOR_HINTS: &[(&str, &str)] = &[
    ("space", "play"),
    ("←→", "seek"),
    ("i", "in"),
    ("o", "out"),
    ("e", "export"),
    ("ctrl+p", "commands"),
    ("q", "quit"),
];

/// Short key hints for the file browser.
pub(super) const HOME_HINTS: &[(&str, &str)] = &[
    ("enter", "open"),
    ("tab", "into folder"),
    ("↑↓", "select"),
    ("backspace", "up a folder"),
    ("ctrl+p", "commands"),
    ("esc", "quit"),
];

/// What a key does in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum EditAction {
    Quit,
    TogglePlayback,
    Seek(i64),
    StepFrames(i64),
    JumpToStart,
    JumpToEnd,
    MarkIn,
    MarkOut,
    ClearIn,
    SelectPrevious,
    SelectNext,
    JumpToSelected,
    DeleteSelected,
    OpenExport,
    Browse,
    ToggleHelp,
}

/// What a key does in the export dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum DialogAction {
    ToggleMode,
    Confirm,
    Cancel,
}

/// What a key does on the home screen, where typing searches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HomeAction {
    Previous,
    Next,
    /// Open the selected video or folder, or the typed path.
    Open,
    /// Go into the selected folder.
    EnterFolder,
    /// Delete a search character, or go up a folder when the search is empty.
    Backspace,
    /// Clear the search, or quit when it is empty.
    ClearOrQuit,
    Quit,
    Type(char),
}

/// What a key does in the command palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PaletteKey {
    Previous,
    Next,
    Run,
    Close,
    Backspace,
    Type(char),
}

/// `Ctrl+P` opens (and closes) the command palette.
pub(super) fn is_ctrl_p(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('p')
}

pub(super) fn palette_key(key: KeyEvent) -> Option<PaletteKey> {
    if is_ctrl_c(key) || is_ctrl_p(key) {
        return Some(PaletteKey::Close);
    }
    let action = match key.code {
        KeyCode::Up => PaletteKey::Previous,
        KeyCode::Down => PaletteKey::Next,
        KeyCode::Enter => PaletteKey::Run,
        KeyCode::Esc => PaletteKey::Close,
        KeyCode::Backspace => PaletteKey::Backspace,
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            PaletteKey::Type(character)
        }
        _ => return None,
    };
    Some(action)
}

fn is_ctrl_c(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c')
}

pub(super) fn edit_action(key: KeyEvent) -> Option<EditAction> {
    if is_ctrl_c(key) {
        return Some(EditAction::Quit);
    }
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let action = match key.code {
        KeyCode::Char(' ') => EditAction::TogglePlayback,
        KeyCode::Left if shift => EditAction::Seek(-SEEK_LARGE_MICROS),
        KeyCode::Right if shift => EditAction::Seek(SEEK_LARGE_MICROS),
        KeyCode::Left => EditAction::Seek(-SEEK_SMALL_MICROS),
        KeyCode::Right => EditAction::Seek(SEEK_SMALL_MICROS),
        KeyCode::Char(',') => EditAction::StepFrames(-1),
        KeyCode::Char('.') => EditAction::StepFrames(1),
        KeyCode::Home => EditAction::JumpToStart,
        KeyCode::End => EditAction::JumpToEnd,
        KeyCode::Char('i') => EditAction::MarkIn,
        KeyCode::Char('o') => EditAction::MarkOut,
        KeyCode::Esc => EditAction::ClearIn,
        KeyCode::Up | KeyCode::Char('k') => EditAction::SelectPrevious,
        KeyCode::Down | KeyCode::Char('j') => EditAction::SelectNext,
        KeyCode::Enter => EditAction::JumpToSelected,
        KeyCode::Char('d') | KeyCode::Delete => EditAction::DeleteSelected,
        KeyCode::Char('e') => EditAction::OpenExport,
        KeyCode::Char('b') => EditAction::Browse,
        KeyCode::Char('?') => EditAction::ToggleHelp,
        KeyCode::Char('q') => EditAction::Quit,
        _ => return None,
    };
    Some(action)
}

pub(super) fn dialog_action(key: KeyEvent) -> Option<DialogAction> {
    match key.code {
        KeyCode::Tab | KeyCode::BackTab | KeyCode::Left | KeyCode::Right | KeyCode::Char('m') => {
            Some(DialogAction::ToggleMode)
        }
        KeyCode::Enter => Some(DialogAction::Confirm),
        KeyCode::Esc | KeyCode::Char('q') => Some(DialogAction::Cancel),
        _ if is_ctrl_c(key) => Some(DialogAction::Cancel),
        _ => None,
    }
}

pub(super) fn home_action(key: KeyEvent) -> Option<HomeAction> {
    if is_ctrl_c(key) {
        return Some(HomeAction::Quit);
    }
    let action = match key.code {
        KeyCode::Up => HomeAction::Previous,
        KeyCode::Down => HomeAction::Next,
        KeyCode::Enter => HomeAction::Open,
        KeyCode::Tab | KeyCode::Right => HomeAction::EnterFolder,
        KeyCode::Backspace => HomeAction::Backspace,
        KeyCode::Esc => HomeAction::ClearOrQuit,
        KeyCode::Char(character)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            HomeAction::Type(character)
        }
        _ => return None,
    };
    Some(action)
}

/// `Some(true)` for yes, `Some(false)` for no, `None` for other keys.
pub(super) fn confirm_answer(key: KeyEvent) -> Option<bool> {
    match key.code {
        KeyCode::Char('y' | 'Y') => Some(true),
        KeyCode::Char('n' | 'N') | KeyCode::Esc => Some(false),
        _ if is_ctrl_c(key) => Some(false),
        _ => None,
    }
}

/// Whether a key cancels a running export.
pub(super) fn cancels_export(key: KeyEvent) -> bool {
    key.code == KeyCode::Esc || key.code == KeyCode::Char('c') || is_ctrl_c(key)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn shifted(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::SHIFT)
    }

    fn ctrl_c() -> KeyEvent {
        KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)
    }

    #[test]
    fn arrows_seek_and_shift_seeks_further() {
        assert_eq!(
            edit_action(key(KeyCode::Right)),
            Some(EditAction::Seek(SEEK_SMALL_MICROS))
        );
        assert_eq!(
            edit_action(shifted(KeyCode::Left)),
            Some(EditAction::Seek(-SEEK_LARGE_MICROS))
        );
    }

    #[test]
    fn marking_keys_map_to_in_and_out() {
        assert_eq!(
            edit_action(key(KeyCode::Char('i'))),
            Some(EditAction::MarkIn)
        );
        assert_eq!(
            edit_action(key(KeyCode::Char('o'))),
            Some(EditAction::MarkOut)
        );
        assert_eq!(edit_action(key(KeyCode::Esc)), Some(EditAction::ClearIn));
    }

    #[test]
    fn ctrl_c_quits_or_cancels_everywhere() {
        assert_eq!(edit_action(ctrl_c()), Some(EditAction::Quit));
        assert_eq!(home_action(ctrl_c()), Some(HomeAction::Quit));
        assert_eq!(dialog_action(ctrl_c()), Some(DialogAction::Cancel));
        assert_eq!(confirm_answer(ctrl_c()), Some(false));
        assert!(cancels_export(ctrl_c()));
    }

    #[test]
    fn home_keys_type_letters_and_navigate_with_arrows() {
        assert_eq!(
            home_action(key(KeyCode::Char('q'))),
            Some(HomeAction::Type('q'))
        );
        assert_eq!(
            home_action(shifted(KeyCode::Char('B'))),
            Some(HomeAction::Type('B'))
        );
        assert_eq!(home_action(key(KeyCode::Down)), Some(HomeAction::Next));
        assert_eq!(
            home_action(key(KeyCode::Tab)),
            Some(HomeAction::EnterFolder)
        );
        assert_eq!(
            home_action(key(KeyCode::Esc)),
            Some(HomeAction::ClearOrQuit)
        );
        let ctrl_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(home_action(ctrl_x), None, "control keys are not typed");
    }

    #[test]
    fn palette_keys_type_select_run_and_close() {
        let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(is_ctrl_p(ctrl_p));
        assert!(!is_ctrl_p(key(KeyCode::Char('p'))));
        assert_eq!(palette_key(ctrl_p), Some(PaletteKey::Close));
        assert_eq!(palette_key(key(KeyCode::Esc)), Some(PaletteKey::Close));
        assert_eq!(
            palette_key(key(KeyCode::Char('q'))),
            Some(PaletteKey::Type('q'))
        );
        assert_eq!(palette_key(key(KeyCode::Enter)), Some(PaletteKey::Run));
        assert_eq!(palette_key(key(KeyCode::Down)), Some(PaletteKey::Next));
    }

    #[test]
    fn plain_c_in_the_editor_is_not_quit() {
        assert_eq!(edit_action(key(KeyCode::Char('c'))), None);
    }

    #[test]
    fn confirm_accepts_only_yes_or_no() {
        assert_eq!(confirm_answer(key(KeyCode::Char('Y'))), Some(true));
        assert_eq!(confirm_answer(key(KeyCode::Char('n'))), Some(false));
        assert_eq!(confirm_answer(key(KeyCode::Enter)), None);
    }

    #[test]
    fn help_lists_every_editor_binding_once() {
        let keys: Vec<&str> = EDITOR_HELP.iter().map(|(keys, _)| *keys).collect();
        let mut unique = keys.clone();
        unique.sort_unstable();
        unique.dedup();
        assert_eq!(keys.len(), unique.len());
        assert!(keys.contains(&"e"));
    }
}
