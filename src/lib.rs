//! TUI-ClipEditor: a terminal video clip editor built on ffmpeg.
//!
//! The binary in `main.rs` only wires these modules together, so the logic
//! can be tested without a terminal.
//!
//! - Domain logic (no I/O): [`timecode`], [`media`], [`clip`], [`editor`], [`export`].
//! - Adapter for the external ffmpeg programs: [`ffmpeg`].
//! - Command-line interface: [`cli`]; terminal UI: [`tui`].

pub mod cli;
pub mod clip;
pub mod editor;
pub mod export;
pub mod ffmpeg;
pub mod media;
pub mod timecode;
pub mod tui;
