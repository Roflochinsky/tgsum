#!/usr/bin/env bash
# Shared local/CI checks. Run from any directory; requires Bash (Git Bash on Windows).
set -euo pipefail

usage() {
  cat <<'EOF'
Usage: bash scripts/check.sh [full|quick|core|lint|test]

  full   Format, check, Clippy, and tests for the workspace (default).
  quick  Compiler/type/borrow checks for all workspace targets.
  core   Workspace formatting, then check, Clippy, and tests for tgsum-core only.
  lint   Workspace formatting and Clippy (used by CI).
  test   Workspace tests, including doctests (used by CI).

Checks use Cargo.lock without updating it and stop at the first failure.
Formatting is checked without modifying files; fix it with cargo fmt --all.
For setup and troubleshooting, see README.md (Проверки для разработки).
EOF
}

if [[ $# -gt 1 ]]; then
  usage >&2
  exit 2
fi

mode=${1:-full}
case "$mode" in
  full|quick|core|lint|test) ;;
  -h|--help|help) usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$repo_root"

run() {
  printf '\n+ %s\n' "$*"
  if "$@"; then
    return 0
  else
    local status=$?
    printf '\nFAILED (exit %s): %s\n' "$status" "$*" >&2
    exit "$status"
  fi
}

scope=(--workspace)
if [[ "$mode" == core ]]; then
  scope=(-p tgsum-core)
fi

format() {
  run cargo fmt --all --check
}

check() {
  run cargo check "${scope[@]}" --all-targets --all-features --locked
}

lint() {
  run cargo clippy "${scope[@]}" --all-targets --all-features --locked -- -D warnings
}

test_rust() {
  # Keep Cargo's default target selection: --all-targets would omit doctests.
  run cargo test "${scope[@]}" --all-features --locked
}

case "$mode" in
  full|core) format; check; lint; test_rust ;;
  quick) check ;;
  lint) format; lint ;;
  test) test_rust ;;
esac

printf '\nPASS: %s\n' "$mode"
