#!/usr/bin/env bash
# Exercises install-user.sh without reading or changing the graphical session.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
sandbox="$(mktemp -d)"
trap 'rm -rf "$sandbox"' EXIT

home="$sandbox/home"
config="$sandbox/config"
data="$sandbox/data"
stub_bin="$sandbox/bin"
mkdir -p "$home" "$config/hypr" "$data" "$stub_bin"

cat >"$stub_bin/systemctl" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$SYSTEMCTL_LOG"
SH
chmod +x "$stub_bin/systemctl"
cat >"$stub_bin/systemd-run" <<'SH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >>"$SYSTEMD_RUN_LOG"
if [[ "${SYSTEMD_RUN_FAIL:-}" == "1" ]]; then
  printf '%s\n' 'synthetic config error'
  exit 1
fi
SH
chmod +x "$stub_bin/systemd-run"

source_bin="$sandbox/opendesk-source"
printf '#!/usr/bin/env bash\nexit 0\n' >"$source_bin"
chmod +x "$source_bin"
hypr_config="$config/hypr/hyprland.lua"
printf '%s\n' 'monitor = "example"' >"$hypr_config"
mkdir -p "$home/.local/bin" "$config/systemd/user" "$config/hypr/conf"
printf '%s\n' 'old-bin' >"$home/.local/bin/opendesk"
printf '%s\n' 'old-unit' >"$config/systemd/user/opendesk.service"
printf '%s\n' 'old-module' >"$config/hypr/conf/opendesk.lua"

run() {
  env HOME="$home" XDG_CONFIG_HOME="$config" XDG_DATA_HOME="$data" \
    OPENDESK_BIN="$source_bin" PATH="$stub_bin:$PATH" SYSTEMCTL_LOG="$sandbox/systemctl.log" SYSTEMD_RUN_LOG="$sandbox/systemd-run.log" \
    "$root/scripts/install-user.sh" "$@"
}

fail() { printf 'FAIL: %s\n' "$*" >&2; exit 1; }
check() { "$@" || fail "$*"; }
count_managed() { grep -Fc 'require("conf/opendesk")  -- managed-by opendesk' "$hypr_config"; }

run install
check cmp -s "$home/.local/bin/opendesk" "$source_bin"
check test -f "$config/systemd/user/opendesk.service"
check test -f "$config/hypr/conf/opendesk.lua"
check grep -Fq 'hl.layer_rule' "$config/hypr/conf/opendesk.lua"
check test "$(count_managed)" -eq 1
backup_pointer="$data/opendesk/backups/latest-hyprland-backup"
check test -f "$backup_pointer"
backup="$(<"$backup_pointer")"
check cmp -s "$backup" <(printf '%s\n' 'monitor = "example"')
first_install_backup="$(<"$data/opendesk/backups/latest-install-backup")"
check test -f "$first_install_backup/replaced-paths"
check grep -Fx "$home/.local/bin/opendesk" "$first_install_backup/replaced-paths"
check grep -Fx "$config/systemd/user/opendesk.service" "$first_install_backup/replaced-paths"
check grep -Fx "$config/hypr/conf/opendesk.lua" "$first_install_backup/replaced-paths"
check grep -Fx "$hypr_config" "$first_install_backup/replaced-paths"

run install
check test "$(count_managed)" -eq 1
check cmp -s "$home/.local/bin/opendesk" "$source_bin"

printf '%s\n' 'mutated after install' >>"$hypr_config"
run rollback-hyprland
check cmp -s "$hypr_config" <(printf '%s\n' 'monitor = "example"')

run uninstall
check cmp -s "$home/.local/bin/opendesk" <(printf '%s\n' 'old-bin')
check cmp -s "$config/systemd/user/opendesk.service" <(printf '%s\n' 'old-unit')
check cmp -s "$config/hypr/conf/opendesk.lua" <(printf '%s\n' 'old-module')
check test "$(count_managed)" -eq 0

failed_home="$sandbox/failed-home"
failed_config="$sandbox/failed-config"
failed_data="$sandbox/failed-data"
mkdir -p "$failed_home" "$failed_config/hypr" "$failed_data"
printf '%s\n' 'unchanged' >"$failed_config/hypr/hyprland.lua"
if env HOME="$failed_home" XDG_CONFIG_HOME="$failed_config" XDG_DATA_HOME="$failed_data" \
  PATH="$stub_bin:$PATH" SYSTEMCTL_LOG="$sandbox/systemctl.log" SYSTEMD_RUN_LOG="$sandbox/systemd-run.log" "$root/scripts/install-user.sh" install; then
  fail 'install without OPENDESK_BIN unexpectedly succeeded'
fi
check cmp -s "$failed_config/hypr/hyprland.lua" <(printf '%s\n' 'unchanged')
check test ! -e "$failed_home/.local/bin/opendesk"
check test ! -e "$failed_config/systemd/user/opendesk.service"
check test ! -e "$failed_config/hypr/conf/opendesk.lua"
check test ! -e "$failed_data/opendesk/backups/latest-hyprland-backup"

rollback_home="$sandbox/rollback-home"
rollback_config="$sandbox/rollback-config"
rollback_data="$sandbox/rollback-data"
mkdir -p "$rollback_home/.local/bin" "$rollback_config/systemd/user" "$rollback_config/hypr/conf" "$rollback_data"
printf '%s\n' 'old-bin-after-failure' >"$rollback_home/.local/bin/opendesk"
printf '%s\n' 'old-unit-after-failure' >"$rollback_config/systemd/user/opendesk.service"
printf '%s\n' 'old-module-after-failure' >"$rollback_config/hypr/conf/opendesk.lua"
printf '%s\n' 'main-before-failure' >"$rollback_config/hypr/hyprland.lua"
printf '%s\n' 'unrelated-user-data' >"$rollback_config/hypr/conf/user.lua"
if env HOME="$rollback_home" XDG_CONFIG_HOME="$rollback_config" XDG_DATA_HOME="$rollback_data" \
  OPENDESK_BIN="$source_bin" PATH="$stub_bin:$PATH" SYSTEMCTL_LOG="$sandbox/systemctl.log" SYSTEMD_RUN_LOG="$sandbox/systemd-run.log" SYSTEMD_RUN_FAIL=1 \
  "$root/scripts/install-user.sh" install; then
  fail 'install with synthetic configerrors failure unexpectedly succeeded'
fi
check cmp -s "$rollback_home/.local/bin/opendesk" <(printf '%s\n' 'old-bin-after-failure')
check cmp -s "$rollback_config/systemd/user/opendesk.service" <(printf '%s\n' 'old-unit-after-failure')
check cmp -s "$rollback_config/hypr/conf/opendesk.lua" <(printf '%s\n' 'old-module-after-failure')
check cmp -s "$rollback_config/hypr/hyprland.lua" <(printf '%s\n' 'main-before-failure')
check cmp -s "$rollback_config/hypr/conf/user.lua" <(printf '%s\n' 'unrelated-user-data')
check test ! -e "$rollback_data/opendesk/backups/latest-install-backup"
check test ! -e "$rollback_data/opendesk/backups/latest-hyprland-backup"

check grep -Fx -- '--user daemon-reload' "$sandbox/systemctl.log"
check grep -Fx -- '--user enable opendesk.service' "$sandbox/systemctl.log"
check grep -Fx -- '--user disable --now opendesk.service' "$sandbox/systemctl.log"
check grep -Fx -- '--user --wait --pipe --collect hyprctl configerrors' "$sandbox/systemd-run.log"
printf 'PASS: installer sandbox idempotence, backup, rollback, restoration and transaction failure recovery\n'
