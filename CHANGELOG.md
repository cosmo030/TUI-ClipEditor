# Changelog

All notable changes are listed here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- `clipedit` terminal clip editor: open a video from the command line or a file browser, preview frames in the terminal (sixel, kitty, iTerm2, or half-blocks), play, seek, and step frames.
- Mark clips with in (`i`) and out (`o`) points; list, select, jump to, and delete clips; timeline showing clips, the in point, and the playhead.
- Export all clips with **Fast copy** (stream copy, keeps the container) or **Precise** (re-encode to H.264/AAC MP4), with progress and cancel. Exports never overwrite existing files.
- `-o/--output-dir` option, `--help`, and `--version`.
- Quality gates: rustfmt, clippy, tests on Ubuntu and Windows, cargo-deny, and gitleaks in CI.
- `-p/--preview <auto|blocks|sixel|kitty|iterm2>` option; the preview title shows the mode in use.
- Smooth playback: while playing, the preview streams frames from one ffmpeg process (up to 30 fps) instead of seeking for every frame. Previews are encoded off the UI thread, centred in the preview box, and up to 1920×1080 pixels.
- Faster sixel playback on large, grainy videos: frames shown while playing use 64 colours without dithering (about half the data; the paused frame is redrawn at full quality), and the playback stream decodes on the GPU where available (`-hwaccel auto`) with a faster scaler.
- While playing, the preview title shows where the time goes: frames reaching the screen per second, frames decoded per second, encode time, draw time, and data per frame.

### Fixed

- Sixel playback flickered: every frame first erased the whole preview before the new image arrived. Sixel frames are now drawn straight over the previous frame (erasing only when the image moves, changes size, or was covered by a dialog), and each screen update is sent as one synchronized frame (DEC mode 2026) on terminals that support it.

- The preview stayed blank in VS Code's terminal (and other terminals guessed from `TERM_PROGRAM`): image formats are now used only when the terminal confirms them or supports them by default; otherwise the preview uses half-blocks.
