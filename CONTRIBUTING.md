# Contributing

Thanks for helping! This file covers the workflow, the checks, and how CI works.

## Prerequisites

| Tool | Version | Notes |
|---|---|---|
| Rust | 1.99.1 | Installed automatically by `rustup` from [`rust-toolchain.toml`](rust-toolchain.toml). Install rustup from <https://rustup.rs> (Windows: `winget install Rustlang.Rustup`). |
| ffmpeg and ffprobe | On `PATH`; tested with 7.1 (Windows), the Ubuntu 24.04 package, and the Homebrew formula (macOS) | Needed to run the app and the integration tests. Windows: `winget install Gyan.FFmpeg`; macOS: `brew install ffmpeg`; Debian/Ubuntu: `sudo apt install ffmpeg`. |
| cargo-deny | 0.20.2 | `cargo install cargo-deny --locked --version 0.20.2` |
| gitleaks | 8.30.1 | Windows: `winget install Gitleaks.Gitleaks`; macOS: `brew install gitleaks`; others: <https://github.com/gitleaks/gitleaks/releases> |
| bash | any | Runs `scripts/check.sh`. On Windows, use Git Bash (comes with Git for Windows). |

On Windows, the Rust MSVC toolchain also needs the "Desktop development with C++" workload from the Visual Studio Build Tools.

## Checks

Run every check before opening a pull request:

```bash
scripts/check.sh
```

Or run one check: `scripts/check.sh fmt|lint|test|build|deny|secrets`.

| Check | Command it runs |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `lint` | `cargo clippy --locked --all-targets --all-features -- -D warnings` |
| `test` | `cargo test --locked` |
| `build` | `cargo build --locked --release` |
| `deny` | `cargo deny --locked check` (vulnerabilities, licenses, sources) |
| `secrets` | `gitleaks git --redact --no-banner .` |

Fix formatting with `cargo fmt --all`. Lint rules and complexity thresholds live in [`Cargo.toml`](Cargo.toml) and [`clippy.toml`](clippy.toml); don't loosen them to make code pass.

## CI

[`.github/workflows/ci.yml`](.github/workflows/ci.yml) runs on every pull request, on pushes to `main`, weekly (to catch new advisories), and on demand. It calls `scripts/check.sh`, so CI runs the same commands as your machine.

| Job | Runs on | Checks |
|---|---|---|
| Format and lint | Ubuntu | `fmt`, `lint` |
| Test and build | Ubuntu and Windows | `test`, `build` (installs ffmpeg first) |
| Dependency and secret scan | Ubuntu | `deny`, `secrets` (full Git history) |

The workflow has read-only repository permissions, uses no secrets, and pins third-party actions to commit SHAs. Dependabot opens monthly update pull requests for Cargo dependencies and the pinned actions.

## Workflow

1. Branch from `main`: `feat/…`, `fix/…`, `docs/…`, `build/…`, `ci/…`, `refactor/…`, `test/…`, `chore/…`.
2. Keep each pull request to one logical change.
3. Write [Conventional Commits](https://www.conventionalcommits.org/): `feat(export): add precise mode`.
4. Add tests for every behavior change, and a regression test for every bug fix.
5. Open a pull request using the template; report exactly which checks you ran.

## Ground rules

- Never commit video or audio files. Tests generate synthetic media with ffmpeg.
- Never commit secrets or personal data.
- New dependencies need a reason in the pull request; frameworks or large dependencies need an ADR in [`docs/adr/`](docs/adr/).
- By contributing, you agree your contributions are dual licensed under MIT OR Apache-2.0.
