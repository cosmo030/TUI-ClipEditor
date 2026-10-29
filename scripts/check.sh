#!/usr/bin/env bash
# Runs the project's quality checks. CI calls this same script, one check per
# step, so local results match CI.
#
# Usage: scripts/check.sh [all|fmt|lint|test|build|deny|secrets]   (default: all)
set -euo pipefail

cd "$(dirname "$0")/.."

checks="fmt lint test build deny secrets"

check_fmt() { cargo fmt --all --check; }
check_lint() { cargo clippy --locked --all-targets --all-features -- -D warnings; }
check_test() { cargo test --locked; }
check_build() { cargo build --locked --release; }
check_deny() { cargo deny --locked check; }
check_secrets() { gitleaks git --redact --no-banner .; }

run() {
  echo "==> $1"
  "check_$1"
}

case "${1:-all}" in
  all)
    for check in $checks; do run "$check"; done
    echo "All checks passed."
    ;;
  fmt | lint | test | build | deny | secrets) run "$1" ;;
  *)
    echo "unknown check: $1 (expected all or one of: $checks)" >&2
    exit 64
    ;;
esac
