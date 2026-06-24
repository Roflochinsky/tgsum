#!/bin/bash
# Double-click launcher. If quarantined by Gatekeeper, run once:
#   xattr -d com.apple.quarantine tgsum.command
cd "$(dirname "$0")" || exit 1
if command -v tgsum >/dev/null 2>&1; then tgsum; else npx tgsum; fi
echo ""; read -r -p "Нажмите Enter чтобы закрыть…" _
