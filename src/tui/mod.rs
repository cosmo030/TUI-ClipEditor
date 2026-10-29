//! The terminal user interface.
//!
//! - [`app`] — UI state and the pure `update(message) -> effects` function.
//! - [`session`], [`browser`] — state of the editing screen and file browser.
//! - [`keys`] — key bindings.
//! - [`view`], [`timeline`] — drawing; [`preview`] — choosing the image protocol.
//! - [`runtime`] — the terminal, the event loop, and background workers; the
//!   only part that performs I/O.

mod app;
mod browser;
mod keys;
mod preview;
mod runtime;
mod session;
mod timeline;
mod view;

use std::io;
use std::path::PathBuf;

use crate::media::MediaInfo;

pub use runtime::run;

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
