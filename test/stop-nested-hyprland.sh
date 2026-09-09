#!/usr/bin/env bash
set -euo pipefail

label="${1:-default}"
state_dir="${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR is not set}/opendesk-nested-$label"

if [[ ! -f "$state_dir/pid" ]]; then
  echo "no nested Hyprland pid file at $state_dir/pid" >&2
  exit 0
fi

pid="$(cat "$state_dir/pid")"
if kill -0 "$pid" 2>/dev/null; then
  kill "$pid"
  for _ in $(seq 1 50); do
    if ! kill -0 "$pid" 2>/dev/null; then
      break
    fi
    sleep 0.1
  done
  if kill -0 "$pid" 2>/dev/null; then
    kill -9 "$pid"
  fi
fi

rm -f "$state_dir/pid" "$state_dir/env"
echo "nested Hyprland $pid stopped"
