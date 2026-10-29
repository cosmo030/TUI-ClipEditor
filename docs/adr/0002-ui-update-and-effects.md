# 0002. UI state changes through `update(message) -> effects`

- **Status:** Proposed (accepted when the owner merges the pull request that adds it)
- **Date:** 2026-10-29
- **Deciders:** cosmo030 (owner)

## Context

The TUI reacts to several sources at once: key presses, a playback clock, preview frames decoded on a worker thread, probe results, and export progress. ratatui only draws; it leaves state management to the application. The engineering standards require UI code to be testable without a terminal, ffmpeg, or a real clock, and business logic to stay out of the UI.

## Decision

We will structure the UI as a message/update/effect loop (the pattern known from The Elm Architecture):

- `tui::app::App` holds all UI state. `App::update(Msg) -> Vec<Effect>` is the only way it changes, and it performs no I/O.
- `Msg` covers everything that can happen: key presses, elapsed time ticks, decoded frames, probe and directory results, export events.
- `Effect` covers everything the UI asks the outside world to do: list a directory, probe a file, decode a frame, start or cancel an export, quit.
- `tui::runtime` owns the terminal, background threads, and the clock; it performs effects and turns their results into messages.
- Editing rules stay in the domain `editor::Editor`; the app translates key actions into editor calls.

## Alternatives Considered

| Option | Pros | Cons | Why not chosen |
|---|---|---|---|
| Handlers that call ffmpeg and the filesystem directly | Less code. | UI logic only testable with real ffmpeg and a terminal; blocking calls freeze the UI. | Violates the testability requirement. |
| Shared state behind `Arc<Mutex<…>>` updated by worker threads | Workers update state directly. | Locking, harder reasoning about order, data races in logic. | Channels and a single owner are simpler. |
| An async runtime (`tokio`) with tasks | Natural for concurrent I/O. | Large dependency; the app has only two kinds of background work. | Threads and one channel suffice (ADR 0001). |

## Consequences

- **Positive:** Every UI flow (marking, export dialog, confirmations, frame scheduling, stale-frame handling) is unit-tested with plain values. The runtime is small and holds no decisions.
- **Negative:** Adding a feature means adding a message or an effect and handling it in two places (app and runtime).
- **Follow-up:** Keep `Msg` and `Effect` small; if the app grows a second large screen, split `update` by screen as `session` and `browser` already do.

## Security and Data Considerations

None.
