//! The terminal user interface.
//!
//! - `app` — UI state and the pure `update(message) -> effects` function.
//! - `session`, `browser` — state of the editing screen and the home screen's
//!   folder search (`fuzzy` ranks matches; `logo` is the home screen wordmark).
//! - `keys` — key bindings; `palette` — the `ctrl+p` command palette.
//! - `view`, `timeline` — drawing; `theme` — the colours they use;
//!   `preview` — choosing the image protocol;
//!   `terminal_image` — encoding and drawing preview images.
//! - `playback`, `encoder` — workers that stream frames while playing and
//!   encode them for the terminal.
//! - `runtime` — the terminal, the event loop, and the other background
//!   workers. Together with the workers above, the only code that performs I/O.

mod app;
mod browser;
mod encoder;
mod fuzzy;
mod keys;
mod logo;
mod palette;
mod playback;
mod preview;
mod runtime;
mod session;
mod terminal_image;
mod theme;
mod timeline;
mod view;

use std::io;
use std::path::PathBuf;

use crate::cli::{PreviewMode, ThemeChoice};
use crate::media::MediaInfo;

pub use runtime::run;

/// Options that shape the UI, from the command line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Settings {
    /// `--output-dir`; `None` uses each video's directory.
    pub output_dir: Option<PathBuf>,
    /// `--preview`.
    pub preview: PreviewMode,
    /// `--theme`.
    pub theme: ThemeChoice,
}

/// What the UI shows first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// The file browser in this directory.
    Browse(PathBuf),
    /// The editor with an already probed video.
    Edit {
        /// The video file.
        path: PathBuf,
        /// Its probed properties.
        media: MediaInfo,
    },
}

/// Errors that end the UI.
#[derive(Debug, thiserror::Error)]
pub enum TuiError {
    /// Drawing to or reading from the terminal failed.
    #[error("terminal error: {0}")]
    Terminal(#[from] io::Error),
}
