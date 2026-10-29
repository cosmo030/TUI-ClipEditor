//! Choosing how preview images are drawn in the terminal.
//!
//! `ratatui-image` guesses some image protocols from environment variables.
//! A wrong guess is invisible: the terminal silently drops the image and the
//! preview stays blank. For example, VS Code sets `TERM_PROGRAM=vscode`, which
//! is taken to mean iTerm2 images work, but VS Code's terminal only shows
//! them when its `terminal.integrated.enableImages` setting is on. So in
//! `auto` mode an image protocol is used only when the terminal confirmed it,
//! or for terminals that support it out of the box; everything else gets
//! half-blocks, which always show. `--preview` overrides the choice.

use std::env;

use ratatui_image::picker::{Capability, Picker, ProtocolType};

use crate::cli::PreviewMode;

/// Terminals (by `TERM_PROGRAM`) that show iTerm2 inline images by default.
const ITERM2_BY_DEFAULT: &[&str] = &["iTerm", "WezTerm"];

/// Builds the picker for `mode`. Must run after the terminal entered the
/// alternate screen and before anything reads terminal events.
pub(super) fn create_picker(mode: PreviewMode) -> Picker {
    if mode == PreviewMode::Blocks {
        return Picker::halfblocks();
    }
    // A terminal that does not answer the query gets the half-block picker,
    // which works everywhere, so the query error itself is not shown.
    let mut picker = Picker::from_query_stdio().unwrap_or_else(|_| Picker::halfblocks());
    let term_program = env::var("TERM_PROGRAM").ok();
    let protocol = choose_protocol(
        mode,
        picker.protocol_type(),
        picker.capabilities(),
        term_program.as_deref(),
    );
    picker.set_protocol_type(protocol);
    picker
}

/// The protocol to draw with, given the user's `mode`, what `ratatui-image`
/// detected, what the terminal confirmed, and `TERM_PROGRAM`.
pub(super) fn choose_protocol(
    mode: PreviewMode,
    detected: ProtocolType,
    capabilities: &[Capability],
    term_program: Option<&str>,
) -> ProtocolType {
    match mode {
        PreviewMode::Blocks => ProtocolType::Halfblocks,
        PreviewMode::Sixel => ProtocolType::Sixel,
        PreviewMode::Kitty => ProtocolType::Kitty,
        PreviewMode::Iterm2 => ProtocolType::Iterm2,
        PreviewMode::Auto => {
            let confirmed = match detected {
                ProtocolType::Kitty => capabilities.contains(&Capability::Kitty),
                ProtocolType::Sixel => capabilities.contains(&Capability::Sixel),
                ProtocolType::Iterm2 => term_program
                    .is_some_and(|name| ITERM2_BY_DEFAULT.iter().any(|known| name.contains(known))),
                ProtocolType::Halfblocks => true,
            };
            if confirmed {
                detected
            } else {
                ProtocolType::Halfblocks
            }
        }
    }
}

/// A short name for the preview title, so users can see which mode is active.
pub(super) const fn protocol_label(protocol: ProtocolType) -> &'static str {
    match protocol {
        ProtocolType::Halfblocks => "blocks",
        ProtocolType::Sixel => "sixel",
        ProtocolType::Kitty => "kitty",
        ProtocolType::Iterm2 => "iterm2",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn auto(
        detected: ProtocolType,
        capabilities: &[Capability],
        term: Option<&str>,
    ) -> ProtocolType {
        choose_protocol(PreviewMode::Auto, detected, capabilities, term)
    }

    #[test]
    fn vscode_guess_falls_back_to_blocks() {
        // Regression: VS Code's terminal drops iTerm2 images unless
        // `terminal.integrated.enableImages` is on, which left the preview blank.
        assert_eq!(
            auto(ProtocolType::Iterm2, &[], Some("vscode")),
            ProtocolType::Halfblocks
        );
    }

    #[test]
    fn iterm2_is_kept_for_terminals_that_support_it_by_default() {
        assert_eq!(
            auto(ProtocolType::Iterm2, &[], Some("iTerm.app")),
            ProtocolType::Iterm2
        );
        assert_eq!(
            auto(ProtocolType::Iterm2, &[], Some("WezTerm")),
            ProtocolType::Iterm2
        );
        assert_eq!(
            auto(ProtocolType::Iterm2, &[], None),
            ProtocolType::Halfblocks
        );
    }

    #[test]
    fn sixel_and_kitty_need_the_terminal_to_confirm_them() {
        assert_eq!(
            auto(ProtocolType::Sixel, &[Capability::Sixel], None),
            ProtocolType::Sixel
        );
        assert_eq!(
            auto(ProtocolType::Kitty, &[Capability::Kitty], Some("kitty")),
            ProtocolType::Kitty
        );
        assert_eq!(
            auto(ProtocolType::Sixel, &[], None),
            ProtocolType::Halfblocks
        );
        assert_eq!(
            auto(ProtocolType::Kitty, &[Capability::Sixel], None),
            ProtocolType::Halfblocks
        );
    }

    #[test]
    fn explicit_modes_override_detection() {
        assert_eq!(
            choose_protocol(
                PreviewMode::Iterm2,
                ProtocolType::Halfblocks,
                &[],
                Some("vscode")
            ),
            ProtocolType::Iterm2
        );
        assert_eq!(
            choose_protocol(
                PreviewMode::Blocks,
                ProtocolType::Kitty,
                &[Capability::Kitty],
                None
            ),
            ProtocolType::Halfblocks
        );
        assert_eq!(
            choose_protocol(PreviewMode::Sixel, ProtocolType::Halfblocks, &[], None),
            ProtocolType::Sixel
        );
        assert_eq!(
            choose_protocol(PreviewMode::Kitty, ProtocolType::Halfblocks, &[], None),
            ProtocolType::Kitty
        );
    }

    #[test]
    fn labels_match_the_cli_mode_names() {
        assert_eq!(protocol_label(ProtocolType::Halfblocks), "blocks");
        assert_eq!(protocol_label(ProtocolType::Iterm2), "iterm2");
    }
}
