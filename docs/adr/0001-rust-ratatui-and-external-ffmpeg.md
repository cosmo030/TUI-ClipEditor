# 0001. Rust, ratatui, and ffmpeg as an external program

- **Status:** Accepted
- **Date:** 2026-10-29
- **Deciders:** cosmo030 (owner)

## Context

The project is a terminal clip editor: open a video, scrub through it with a picture preview inside the terminal, mark in/out points, and export clips. It is open source, single-user, and must work on Windows (the owner's platform) as well as Linux and macOS. Decoding and encoding video is far outside what the project should implement itself; ffmpeg is the de-facto tool and is usually already installed.

## Decision

We will build a single Rust binary (`clipedit`) with these choices:

1. **Language:** Rust (edition 2024), toolchain pinned in `rust-toolchain.toml`.
2. **TUI:** `ratatui` with its default `crossterm` backend, used through ratatui's re-export so the two versions cannot drift apart.
3. **Preview images:** `ratatui-image` (stable 11.x, default features off, `crossterm` feature on). It detects the terminal's graphics protocol (sixel, kitty, iTerm2) and falls back to Unicode half-blocks.
4. **Video work:** run the user's installed `ffmpeg` and `ffprobe` as child processes with argument lists (no shell). ffprobe output is read as JSON with `serde`/`serde_json`. Preview frames are read from ffmpeg as raw RGB bytes into an `image::RgbImage`.
5. **Concurrency:** standard library threads and `std::sync::mpsc` channels; no async runtime.
6. **Errors:** typed error enums with `thiserror`.
7. **No logging framework.** Errors are shown in the TUI and printed on exit. A TUI owns the terminal, so log lines on stderr would corrupt the screen.

### Dependency justifications

| Dependency | Problem | Why this one | License |
|---|---|---|---|
| `ratatui` 0.30 | Drawing a TUI. | The maintained, widely used Rust TUI library; immediate-mode rendering and a test backend for unit tests. | MIT |
| `ratatui-image` 11.1 | Showing video frames in the terminal. | Only maintained ratatui widget that supports sixel/kitty/iTerm2 with a half-block fallback. The 12.x line is still a release candidate, so 11.1 is used. | MIT |
| `image` 0.25 (no default features) | An RGB image type to hand frames to `ratatui-image`. | Already required by `ratatui-image`; default features off keeps codecs out. | MIT OR Apache-2.0 |
| `serde`, `serde_json` | Parsing ffprobe's JSON output into typed structs. | Standard Rust serialization crates. | MIT OR Apache-2.0 |
| `thiserror` 2 | Implementing `std::error::Error` for error enums. | Small, widely used, no runtime cost. | MIT OR Apache-2.0 |
| `tempfile` (dev only) | Temporary directories in integration tests. | Standard, cleans up on drop. | MIT OR Apache-2.0 |

All licenses are permissive and allowed in `deny.toml`. Transitive licenses are checked by `cargo deny`.

## Alternatives Considered

| Option | Pros | Cons | Why not chosen |
|---|---|---|---|
| Python + Textual | Fastest to build; rich widgets. | Needs a Python install; slower frame drawing. | Owner chose Rust for a single binary and smoother preview. |
| Go + Bubble Tea | Single binary; polished TUIs. | Go not installed; weaker image-in-terminal options. | Same as above. |
| Link ffmpeg via `ffmpeg-next` bindings | No subprocess overhead; frame-exact decoding. | Needs ffmpeg development libraries at build time, painful on Windows; licensing gets harder when linking. | Build complexity outweighs the benefit for a clip editor. |
| `clap` for arguments | Generated help, validation. | Large dependency for three options. | A 60-line parser with tests is enough. |
| `tokio` async runtime | Async process I/O. | Large dependency; no network or high fan-out. | Threads and channels cover two background workers. |

## Consequences

- **Positive:** One small binary; nothing to install except ffmpeg. The editor logic is plain Rust and testable without a terminal or ffmpeg.
- **Negative:** Each preview frame costs one ffmpeg process start (roughly 50–200 ms), so playback in the preview is low frame rate. Fast-copy exports start at the nearest keyframe before the in point. ffmpeg must be on `PATH`.
- **Follow-up:** A long-running decoder process for smoother playback; revisit `ratatui-image` 12 when it is stable.

## Security and Data Considerations

The application reads and writes only local files the user selects. ffmpeg is called with argument lists, never through a shell, so file names cannot inject commands. Exports pass `-n` to ffmpeg and check for existing files first, so they never overwrite user files. No network access, no telemetry, no stored data.
