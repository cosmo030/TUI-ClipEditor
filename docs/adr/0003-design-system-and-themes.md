# 0003. Design system and themes

- **Status:** Accepted
- **Date:** 2026-10-30
- **Deciders:** cosmo030 (owner)

## Context

The owner wants the interface to look like OpenCode's terminal UI: dark panels filled with a background colour instead of boxed borders, muted secondary text, one accent colour, a coloured bar beside the active element, and a `ctrl+p` command palette. Colours are currently hard-coded per widget (`Color::Cyan`, `.dim()`, …), so restyling would mean touching every widget, and there is no way to adapt to terminals without 24-bit colour.

OpenCode itself is built with Bun, TypeScript, SolidJS, and OpenTUI. Adopting that stack was considered and rejected (see Alternatives); only the design language is adopted.

## Decision

We will route every colour in the UI through one `Theme` of named tokens, using OpenCode's token names so its design conventions map directly:

| Token | Use |
|---|---|
| `primary` | Main accent: active element bar, title chip, folders, selection text |
| `secondary` | Second accent (reserved for the palette and chips) |
| `accent` | In point and pending range |
| `text`, `text_muted` | Normal and secondary text (hints, empty states) |
| `background`, `background_panel`, `background_element` | Screen, panels, selected rows and inputs |
| `border`, `border_active`, `border_subtle` | Panel borders, focused border, timeline track |
| `success`, `warning`, `error`, `info` | Clips and progress, selected clip, errors, info messages |

Two built-in themes:

- **`dark`** — 24-bit colours matching the owner's OpenCode screenshot. OpenCode's exact default values could not be retrieved, so the values are our own approximation, not a copy.
- **`system`** — the terminal's own ANSI colours and default background, for terminals without 24-bit colour and for users who want their terminal theme.

`--theme auto|dark|system` selects one. `auto` (default) picks `dark` when the terminal advertises 24-bit colour (`COLORTERM=truecolor|24bit`), is Windows Terminal (`WT_SESSION`), or is a known truecolor terminal (`TERM_PROGRAM` of iTerm2, WezTerm, VS Code); otherwise `system`.

Widgets receive the theme as a parameter; no widget names a colour directly. Custom theme files (OpenCode-style JSON parsed with `serde`) are deferred until there is a need.

## Alternatives Considered

| Option | Pros | Cons | Why not chosen |
|---|---|---|---|
| Rewrite on Bun + TypeScript + SolidJS + OpenTUI (OpenCode's stack) | Same toolkit as OpenCode; flexbox layout; reactive components. | Full rewrite of ~6,000 tested lines; needs a JavaScript runtime installed or a much larger bundled binary; per-frame video work moves behind FFI/GC; OpenTUI is pre-1.0 (docs 0.8), documents no video/animation behaviour, and has a reported blank-sixel issue on Windows; our playback tuning (no-erase sixel, lighter playback frames) would have to be rebuilt inside it. | Cost and risk far exceed the benefit; ratatui can draw the same design. |
| Keep per-widget colours | No new code. | Every restyle touches every widget; no fallback for non-truecolor terminals. | Blocks the UI revamp. |
| Custom theme files now | Users can restyle. | Config loading, validation, and docs for a feature nobody asked for yet. | YAGNI; the token set makes it easy to add later. |

## Consequences

- **Positive:** The revamp (home screen, editor restyle, command palette) changes layout code only; colours come from one place. Terminals without 24-bit colour still get a readable UI.
- **Negative:** One more parameter threaded through the view code. The `dark` theme's background fill paints every cell, which only changes the first draw (ratatui sends only changed cells afterwards).
- **Follow-up:** Custom theme files and a theme picker in the command palette, when wanted.

## Security and Data Considerations

None. Only environment variables that describe the terminal are read.
