# AGENTS.md — TUI-ClipEditor

This repository follows the maintainer's engineering standards (version 1.0.0). The standards are kept as a **local, uncommitted copy** in `engineering-standards/` (see [Approved Exceptions](#approved-exceptions)).

**Before making any change, read `engineering-standards/AGENTS.md` and follow its mandatory workflow.** Read the other standards documents as that file directs. If the directory is missing (for example, in a fresh public clone), follow the summary in [Project-Specific Rules](#project-specific-rules).

This file contains the project-specific facts the standards rely on. Where this file is more specific than the standards, it takes precedence, but it cannot override law or security obligations.

## Project Overview

- **Purpose:** A terminal user interface (TUI) for cutting clips out of video files: open a video, scrub with an in-terminal preview, mark in/out points, and export the clips through the user's installed ffmpeg.
- **Owner:** cosmo030 (repository owner; approves exceptions).
- **Tier:** 1 – Small. Reason: a local, single-user desktop tool; it reads and writes only files the user points it at, has no network access, no accounts, and stores no personal data. Two Tier 2 controls are adopted anyway because the project is public: dependency vulnerability scanning and a protected `main` branch.
- **Status:** In development.

## Technology Stack

Decision record: [docs/adr/0001-rust-ratatui-and-external-ffmpeg.md](docs/adr/0001-rust-ratatui-and-external-ffmpeg.md).

| Area | Choice |
|---|---|
| Language and version | Rust 1.99.1, edition 2024 (pinned in `rust-toolchain.toml`) |
| Framework | ratatui 0.30 (crossterm backend), ratatui-image 11 |
| Package manager | Cargo (lockfile: `Cargo.lock`) |
| Database / migrations | None |
| External programs | ffmpeg and ffprobe, found on `PATH` at startup |
| Test framework | Built-in `cargo test` (unit tests in modules, integration tests in `tests/`) |
| Formatter / linter / type checker | rustfmt / clippy (`-D warnings`, rules in `Cargo.toml` and `clippy.toml`) / rustc |
| Security scanning | cargo-deny (advisories, licenses, sources), gitleaks (secrets) |
| CI/CD platform | GitHub Actions (`.github/workflows/ci.yml`); Dependabot for updates |
| Hosting / deployment target | None; users run the binary locally |

## Commands

Run all commands from the repository root. On Windows, run `scripts/check.sh` from Git Bash. Tool prerequisites are listed in [CONTRIBUTING.md](CONTRIBUTING.md#prerequisites).

| Purpose | Command |
|---|---|
| Install dependencies | `cargo fetch --locked` |
| Run the application locally | `cargo run -- [VIDEO]` |
| Format (write) | `cargo fmt --all` |
| Format (check only) | `scripts/check.sh fmt` |
| Lint and type check | `scripts/check.sh lint` |
| Unit and integration tests (need ffmpeg) | `scripts/check.sh test` |
| Build | `scripts/check.sh build` |
| Dependency scan | `scripts/check.sh deny` |
| Secret scan | `scripts/check.sh secrets` |
| **All checks (what CI runs)** | `scripts/check.sh` |

## Repository Layout

```text
src/             Library modules and the thin `clipedit` binary (src/main.rs)
tests/           Integration tests (run real ffmpeg on generated media)
scripts/         check.sh: the single entry point for all checks
docs/adr/        Architecture decision records
.github/         CI workflow, Dependabot, pull request template
```

## Architecture Notes

- Overview: [docs/architecture.md](docs/architecture.md). ADRs: [docs/adr/](docs/adr/).
- Domain modules (`timecode`, `media`, `clip`, `editor`, `export`) do no I/O and read no clock; keep them that way.
- Everything ffmpeg-specific lives in `src/ffmpeg/`; translate its output into domain types there.
- UI state changes only through `tui::app::App::update(Msg) -> Vec<Effect>` ([ADR 0002](docs/adr/0002-ui-update-and-effects.md)); `tui::runtime` performs the effects. New UI features follow `tui/session.rs` as the reference.
- Key bindings live in `tui/keys.rs`, together with the help text; update the README key table when they change.
- Colours come only from `tui::theme::Theme` tokens ([ADR 0003](docs/adr/0003-design-system-and-themes.md)); widgets never name a colour directly.

## Environments and Deployment

- Environments: development (local) and CI (GitHub Actions). There is no server deployment; users build or download the binary and run it locally.
- Agents MUST NOT publish releases, crates, or packages unless explicitly authorized for that specific action.

## Sensitive Areas

Changes in these areas are high-risk and require a plan before implementation:

- `.github/` — CI workflows and repository automation.
- Code that builds ffmpeg command lines or writes, renames, or deletes files on disk (exports must never overwrite or delete the user's files).

## Project-Specific Rules

- The engineering standards directory MUST NOT be committed or pushed. It is listed in `.gitignore`.
- Video and audio files MUST NOT be committed. Tests generate synthetic media with ffmpeg at runtime.
- ffmpeg is invoked as an external program with arguments passed as a list (never through a shell).
- Exports MUST NOT overwrite existing files.
- Agents may create branches, commit to them, push them, and open pull requests. Agents MUST NOT merge pull requests: the owner reviews and merges.
- Summary of the standards for contributors without the local copy: Conventional Commits; one logical change per pull request; formatter, linter, and tests must pass; tests for every behavior change and a regression test for every bug fix; no secrets or personal data anywhere in the repository; no new dependency without a stated reason.

## Approved Exceptions

| Rule | Deviation | Reason | Scope | Approved by | Review by |
|---|---|---|---|---|---|
| README › Adopting the Standards (copy the standards to `docs/engineering-standards/` in the repository) | The standards stay in a local, gitignored `engineering-standards/` directory and are never committed or pushed. | Owner instruction: the standards are not published with this open-source project. | Whole repository | cosmo030, 2026-10-29 | 2027-04-29 |
| GIT_WORKFLOW › Rules for AI Agents (never commit directly to the default branch) | One bootstrap commit (licenses, README, `.gitignore`, this file) is made directly on `main`. | A new repository needs an initial commit on `main` before branches and pull requests can work. | The first commit only | cosmo030, 2026-10-29 | Not needed (one-time) |

## Known Gaps

- No code coverage measurement in CI.
- macOS is not in the CI matrix.
- `tui::runtime` (terminal setup, input thread, drawing loop) has no automated test; it is checked by running the app. Its export worker is tested.
- No release binaries are published; users build with `cargo install`.
