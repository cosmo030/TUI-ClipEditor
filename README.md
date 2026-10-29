# TUI-ClipEditor

A terminal video clip editor: open a video, scrub through it with a picture preview right in the terminal, mark the parts you want, and export them as separate files with ffmpeg.

**Status:** In development (0.1.0, unreleased) · **Tier:** 1 (see [AGENTS.md](AGENTS.md))

```text
 clipedit  holiday.mp4 · 1920×1080 · 29.97 fps · 00:05:12.300
┌ Preview ──────────────────────────────┐┌ Clips (2) ──────────────────────────────┐
│                                       ││ 1 00:00:12.000 → 00:00:18.500    6.5s   │
│        (the current frame)            ││ 2 00:02:01.250 → 00:02:09.000    7.8s   │
│                                       ││                                         │
└───────────────────────────────────────┘└─────────────────────────────────────────┘
┌ 00:02:03.100 / 00:05:12.300 ──────────────────────────────────────────────────────┐
│───██───────────────────█┃█────────────────────────────────────────────────────────│
└───────────────────────────────────────────────────────────────────────────────────┘
 ⏸ Paused  Added clip 00:02:01.250 – 00:02:09.000 (2 clips in total)
```

## Quick Start

1. Install [ffmpeg](https://ffmpeg.org/download.html) so that `ffmpeg` and `ffprobe` are on your `PATH`.
   - Windows: `winget install Gyan.FFmpeg`
   - macOS: `brew install ffmpeg`
   - Debian/Ubuntu: `sudo apt install ffmpeg`
2. Install Rust 1.99 or newer with [rustup](https://rustup.rs) (Windows: `winget install Rustlang.Rustup`; it also needs the Visual Studio C++ build tools).
3. Build and install `clipedit`:

   ```bash
   cargo install --locked --git https://github.com/cosmo030/TUI-ClipEditor
   ```

4. Open a video:

   ```bash
   clipedit holiday.mp4
   ```

## Usage

```text
clipedit [OPTIONS] [VIDEO]

  VIDEO                   Video to open. Without it (or with a folder), a file browser opens.
  -o, --output-dir <DIR>  Write exported clips to DIR (default: the video's folder)
  -p, --preview <MODE>    How to draw the preview: auto (default), blocks, sixel, kitty, or iterm2
  -h, --help              Print help
  -V, --version           Print the version
```

### Marking clips

Move to where a clip should start and press `i`, then move to where it should end and press `o`. The clip appears in the list and on the timeline. Repeat for as many clips as you like; clips may overlap.

| Key | Action |
|---|---|
| `Space` | Play / pause |
| `←` `→` | Seek 1 second |
| `Shift` + `←` `→` | Seek 10 seconds |
| `,` `.` | Previous / next frame |
| `Home` `End` | Jump to start / end |
| `i` | Set the in point |
| `o` | Set the out point (adds the clip) |
| `Esc` | Clear the in point |
| `↑` `↓` (or `k` `j`) | Select a clip |
| `Enter` | Jump to the selected clip |
| `d` / `Delete` | Delete the selected clip |
| `e` | Export clips |
| `b` | Browse for another video |
| `?` | Show all keys |
| `q` / `Ctrl+C` | Quit (asks first if clips were not exported) |

### Exporting

Press `e`, choose a mode with `Tab`, and press `Enter`.

| Mode | What it does | Use it when |
|---|---|---|
| **Fast copy** (default) | Copies the original streams without re-encoding. Instant and lossless, but a clip starts at the keyframe at or before its in point, so it can begin up to a few seconds early. Keeps the input's container (`.mkv` stays `.mkv`). | You want speed and original quality. |
| **Precise** | Re-encodes to H.264 video and AAC audio in an `.mp4`. Cuts on the exact frame; takes longer; high quality but not lossless. | The cut points must be exact. |

Each clip is written as `<video name>_<start>_<end>.<ext>`, for example `holiday_00-02-01.250_00-02-09.000.mp4`. **Existing files are never overwritten**: if the name is taken, ` (2)`, ` (3)`, … is added. A cancelled or failed export removes its unfinished file.

### The preview

The preview title shows how the picture is drawn, for example `Preview · blocks`.

- **`auto`** (default) uses sharp images only when the terminal confirms it supports them (sixel or the kitty graphics protocol), or for terminals that show iTerm2 inline images out of the box (iTerm2, WezTerm). Everything else gets a coarser picture made of colored half-block characters, which works in every terminal.
- **`--preview sixel`, `kitty`, or `iterm2`** forces that image format, for terminals that support it but are not detected.
- **`--preview blocks`** always uses half-blocks.

Each preview frame is decoded by ffmpeg on demand, so playback in the preview is smooth enough to find a spot but not full frame rate.

## Troubleshooting

### `ffmpeg was not found; install ffmpeg and make sure ffmpeg is on PATH`

**Cause:** ffmpeg or ffprobe is not installed, or its folder is not on `PATH`.
**Fix:** Install ffmpeg (see [Quick Start](#quick-start)), open a new terminal, and check that `ffmpeg -version` and `ffprobe -version` both work.

### The preview is empty

**Cause:** An image format was forced with `--preview` that the terminal does not display. Terminals drop images they cannot show without any error.
**Fix:** Run without `--preview` (or with `--preview blocks`). The preview title shows the mode in use.

### The preview looks blocky

**Cause:** The terminal did not confirm support for sixel, kitty, or iTerm2 images, so half-blocks are used.
**Fix:** Use a terminal with graphics support (Windows Terminal 1.22 or newer, WezTerm, kitty, foot), or force a format you know works with `--preview sixel`, `kitty`, or `iterm2`. In VS Code's terminal, turn on the `terminal.integrated.enableImages` setting first, then use `--preview iterm2`.

### A fast-copy clip starts too early

**Cause:** Stream copy can only start at a keyframe.
**Fix:** Export with **Precise** mode.

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for prerequisites, the single check command (`scripts/check.sh`), and how CI works. The architecture is described in [docs/architecture.md](docs/architecture.md), decisions in [docs/adr/](docs/adr/), and changes in [CHANGELOG.md](CHANGELOG.md).

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you, as defined in the Apache-2.0 license, shall be dual licensed as above, without any additional terms or conditions.
