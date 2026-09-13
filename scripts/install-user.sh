#!/usr/bin/env bash
set -euo pipefail

config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
bin_home="$HOME/.local/bin"
unit_home="$config_home/systemd/user"
hypr_home="$config_home/hypr"
module="$hypr_home/conf/opendesk.lua"
hypr_config="$hypr_home/hyprland.lua"
backup_home="$data_home/opendesk/backups"
unit="$unit_home/opendesk.service"
script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
root="$(cd "$script_dir/.." && pwd)"

fail() { printf 'opendesk install: %s\n' "$*" >&2; exit 1; }

backup_dir=""
declare -a replaced=()
service_was_enabled=false
pending_hyprland_backup=""

has_graphical_session() {
  command -v hyprctl >/dev/null && systemctl --user is-active --quiet graphical-session.target
}

backup_path() {
  local path="$1" backup
  [[ -n "$backup_dir" ]] || return
  backup="$backup_dir${path}"
  mkdir -p "$(dirname "$backup")"
  if [[ -e "$path" || -L "$path" ]]; then
    cp -a "$path" "$backup"
    replaced+=("$path")
  else
    replaced+=("!$path")
  fi
}

restore_replaced() {
  local item path
  for item in "${replaced[@]}"; do
    path="${item#!}"
    rm -rf "$path"
    [[ "$item" == "!"* ]] || cp -a "$backup_dir${path}" "$path"
  done
  systemctl --user daemon-reload || true
  if "$service_was_enabled"; then
    systemctl --user enable opendesk.service || true
  else
    systemctl --user disable opendesk.service || true
  fi
  if has_graphical_session; then
    systemd-run --user --wait --pipe --collect hyprctl reload || true
  fi
}

case "${1:-install}" in
  install)
    [[ ! -L "$hypr_config" ]] || fail "hyprland.lua must be a regular file; symlink targets cannot be restored safely"
    source_bin="${OPENDESK_BIN:-$(command -v opendesk || true)}"
    [[ -n "$source_bin" && -x "$source_bin" ]] || fail "set OPENDESK_BIN to the built opendesk executable"
    mkdir -p "$bin_home" "$unit_home" "$hypr_home/conf" "$backup_home"
    backup_dir="$(mktemp -d "$backup_home/install.XXXXXX")"
    trap 'status=$?; if (( status != 0 )); then restore_replaced; fi' EXIT
    if systemctl --user is-enabled --quiet opendesk.service; then
      service_was_enabled=true
    fi
    backup_path "$bin_home/opendesk"
    backup_path "$unit"
    backup_path "$module"
    if [[ "$(readlink -f "$source_bin")" != "$bin_home/opendesk" ]]; then
      install -m 0755 "$source_bin" "$bin_home/opendesk"
    fi
    install -m 0644 "$root/packaging/opendesk.service" "$unit"

    if [[ -f "$hypr_config" ]] && ! grep -Fqx 'require("conf/opendesk")  -- managed-by opendesk' "$hypr_config"; then
      backup_path "$hypr_config"
      printf '\nrequire("conf/opendesk")  -- managed-by opendesk\n' >> "$hypr_config"
      pending_hyprland_backup="$backup_dir${hypr_config}"
    fi
    cat > "$module" <<'LUA'
-- Managed by opendesk. UWSM starts opendesk.service through graphical-session.target.
-- This module deliberately contains no environment variables or session commands.
hl.layer_rule({
    name = "opendesk-no-animation",
    match = { namespace = "^opendesk-bar$" },
    no_anim = true,
})
hl.bind("mouse:272", hl.dsp.event("opendesk-left-release"), {
    release = true,
    non_consuming = true,
    dont_inhibit = true,
    allow_input_capture = true,
})
hl.bind("CTRL + ALT + ESCAPE", hl.dsp.event("opendesk-emergency-release"), {
    non_consuming = true,
    dont_inhibit = true,
    allow_input_capture = true,
})
return {}
LUA

    systemctl --user daemon-reload
    systemctl --user enable opendesk.service
    if has_graphical_session; then
      systemd-run --user --wait --pipe --collect hyprctl reload
      errors="$(systemd-run --user --wait --pipe --collect hyprctl configerrors)"
      [[ -z "$errors" ]] || fail "Hyprland rejected its configuration: $errors"
    fi
    printf '%s\n' "${replaced[@]}" > "$backup_dir/replaced-paths"
    if [[ -n "$pending_hyprland_backup" && ! -f "$backup_home/latest-hyprland-backup" ]]; then
      printf '%s\n' "$pending_hyprland_backup" > "$backup_home/latest-hyprland-backup"
    fi
    if [[ ! -f "$backup_home/latest-install-backup" ]]; then
      printf '%s\n' "$backup_dir" > "$backup_home/latest-install-backup"
    fi
    trap - EXIT
    printf 'Installed. Start with: systemctl --user start opendesk.service\n'
    ;;
  uninstall)
    systemctl --user disable --now opendesk.service 2>/dev/null || true
    if [[ -f "$backup_home/latest-install-backup" ]]; then
      backup_dir="$(<"$backup_home/latest-install-backup")"
      if [[ -f "$backup_dir/replaced-paths" ]]; then
        mapfile -t replaced < "$backup_dir/replaced-paths"
        for item in "${replaced[@]}"; do
          path="${item#!}"
          case "$path" in
            "$bin_home/opendesk"|"$unit"|"$module")
              rm -rf "$path"
              [[ "$item" == "!"* ]] || cp -a "$backup_dir${path}" "$path"
              ;;
          esac
        done
      else
        rm -f "$unit" "$bin_home/opendesk"
      fi
    else
      rm -f "$unit" "$bin_home/opendesk"
    fi
    if [[ -f "$hypr_config" ]]; then
      sed -i '\|^require("conf/opendesk")  -- managed-by opendesk$|d' "$hypr_config"
    fi
    if [[ -f "$module" ]] && grep -Fq 'Managed by opendesk' "$module"; then
      rm -f "$module"
    fi
    rm -f "$backup_home/latest-install-backup" "$backup_home/latest-hyprland-backup"
    systemctl --user daemon-reload
    printf 'Removed launcher, service and managed Hyprland module. Identity, pairing and config files were preserved.\n'
    ;;
  rollback-hyprland)
    [[ -f "$backup_home/latest-hyprland-backup" ]] || fail "no Hyprland backup is recorded"
    backup="$(<"$backup_home/latest-hyprland-backup")"
    [[ -f "$backup" ]] || fail "recorded backup no longer exists: $backup"
    cp -p "$backup" "$hypr_config"
    printf 'Restored %s\n' "$backup"
    ;;
  *) fail "usage: $0 [install|uninstall|rollback-hyprland]" ;;
esac
