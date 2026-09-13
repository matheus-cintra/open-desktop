#!/usr/bin/env bash
set -euo pipefail
[[ $EUID == 0 ]] || { echo 'Run this installer as root (sudo).'; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
case "${1:-install}" in
install)
  echo 'Installing a dedicated service that detects physical activity. No key codes or typed content are sent to Open Desktop.'
  command -v python3 >/dev/null
  command -v loginctl >/dev/null
  getent group opendesk-activity >/dev/null || groupadd --system opendesk-activity
  getent passwd opendesk-activity >/dev/null || useradd --system --gid opendesk-activity --no-create-home --shell /usr/bin/nologin opendesk-activity
  install -Dm755 "$root/packaging/activity/opendesk-activity.py" /usr/local/libexec/opendesk-activity.py
  install -Dm644 "$root/packaging/activity/opendesk-activity.service" /etc/systemd/system/opendesk-activity.service
  systemctl daemon-reload
  systemctl enable --now opendesk-activity.service
  systemctl restart opendesk-activity.service
  ;;
uninstall)
  systemctl disable --now opendesk-activity.service || true
  rm -f /usr/local/libexec/opendesk-activity.py /etc/systemd/system/opendesk-activity.service
  systemctl daemon-reload
  # Keep the system UID reserved rather than reassigning it to another service.
  ;;
*) echo 'Usage: install-activity.sh [install|uninstall]'; exit 1;;
esac
