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

Rust with ratatui, calling the user's installed ffmpeg. Details and versions are recorded with the project scaffold.

## Commands

Added together with the project scaffold.

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

- None yet.
