#!/bin/bash
set -euo pipefail
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo 'Install on Apple Silicon macOS'; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
exec /usr/bin/python3 "$root/scripts/macos-update.py" "$@"
