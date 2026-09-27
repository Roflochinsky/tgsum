#!/usr/bin/env bash
# Stage Linux helpers with Tauri's required target suffix. This does not
# execute agents, inspect credentials, or build/publish an installer.
set -euo pipefail
if [[ $# -lt 1 || $# -gt 2 || $1 != x86_64-unknown-linux-gnu ]]; then
  echo 'Usage: bash scripts/build-relay.sh x86_64-unknown-linux-gnu [release|debug]' >&2
  exit 2
fi
target=$1
profile=${2:-release}
case "$profile" in
  release) flags=(--release) ;;
  debug) flags=() ;;
  *) echo 'Profile must be release or debug' >&2; exit 2 ;;
esac
repo_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd -- "$repo_root"
target_dir=${CARGO_TARGET_DIR:-target}
cargo build --locked -p tgsum-runner --bin tgsum-codex-relay --bin tgsum-claude-relay --target "$target" "${flags[@]}"
mkdir -p src-tauri/binaries
for relay in tgsum-codex-relay tgsum-claude-relay; do
  cp -- "$target_dir/$target/$profile/$relay" "src-tauri/binaries/$relay-$target"
done
