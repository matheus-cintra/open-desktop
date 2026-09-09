#!/usr/bin/env bash
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
config="$root/test/hyprland-nested.conf"
label="${1:-default}"
state_dir="${XDG_RUNTIME_DIR:?XDG_RUNTIME_DIR is not set}/opendesk-nested-$label"
log_file="$state_dir/hyprland.log"
host_display="${WAYLAND_DISPLAY:?WAYLAND_DISPLAY is not set}"

mkdir -p "$state_dir"

instances_before="$(hyprctl instances -j | jq -r '.[].instance' | sort)"
sockets_before="$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -printf '%f\n' | sort)"

WAYLAND_DISPLAY="$host_display" setsid Hyprland -c "$config" >"$log_file" 2>&1 &
pid=$!
echo "$pid" >"$state_dir/pid"

new_socket=""
new_instance=""
for _ in $(seq 1 100); do
  sleep 0.1
  if ! kill -0 "$pid" 2>/dev/null; then
    echo "nested Hyprland exited early, log follows" >&2
    cat "$log_file" >&2
    exit 1
  fi
  sockets_after="$(find "$XDG_RUNTIME_DIR" -maxdepth 1 -name 'wayland-*' ! -name '*.lock' -printf '%f\n' | sort)"
  new_socket="$(comm -13 <(echo "$sockets_before") <(echo "$sockets_after") | head -n 1)"
  instances_after="$(hyprctl instances -j | jq -r '.[].instance' | sort)"
  new_instance="$(comm -13 <(echo "$instances_before") <(echo "$instances_after") | head -n 1)"
  if [[ -n "$new_socket" && -n "$new_instance" ]]; then
    break
  fi
done

if [[ -z "$new_socket" || -z "$new_instance" ]]; then
  echo "nested Hyprland did not expose a socket within 10 s, log follows" >&2
  cat "$log_file" >&2
  exit 1
fi

for _ in $(seq 1 50); do
  if HYPRLAND_INSTANCE_SIGNATURE="$new_instance" hyprctl monitors -j >/dev/null 2>&1; then
    break
  fi
  sleep 0.1
done

printf 'export WAYLAND_DISPLAY=%s\nexport HYPRLAND_INSTANCE_SIGNATURE=%s\n' "$new_socket" "$new_instance" | tee "$state_dir/env"
echo "log: $log_file" >&2
