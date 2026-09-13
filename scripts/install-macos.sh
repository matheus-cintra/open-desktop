#!/bin/bash
set -euo pipefail
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo 'Install on Apple Silicon macOS'; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
source_app="$root/dist/Open Desktop.app"
target_app="$HOME/Applications/Open Desktop.app"
[[ -x "$source_app/Contents/MacOS/opendesk" ]] || { echo 'Run scripts/build-macos.sh first'; exit 1; }
/usr/bin/codesign --verify --strict "$source_app"
mkdir -p "$HOME/Applications" "$HOME/.local/bin"
stage="$(mktemp -d "$HOME/Applications/.opendesk-install.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
/usr/bin/ditto "$source_app" "$stage/Open Desktop.app"
/usr/bin/codesign --verify --strict "$stage/Open Desktop.app"
backup_root="$HOME/Library/Application Support/Open Desktop/backups"
mkdir -p "$backup_root"
backup="$(mktemp -d "$backup_root/update.XXXXXX")"
[[ ! -d "$HOME/Library/Application Support/opendesk" ]] || /usr/bin/ditto "$HOME/Library/Application Support/opendesk" "$backup/config"
[[ ! -e "$target_app" ]] || /usr/bin/ditto "$target_app" "$backup/Open Desktop.app"
echo "Backup: $backup"
old_requirement=""
if [[ -e "$target_app" ]]; then
  old_requirement="$(/usr/bin/codesign -d -r- "$target_app" 2>&1 | sed -n 's/^# //; s/^designated => //p')"
  # Terminate through the app delegate to release input before replacement.
  if ! /usr/bin/osascript -e 'with timeout of 5 seconds' \
    -e 'if application id "dev.mcintra.opendesk" is running then tell application id "dev.mcintra.opendesk" to quit' \
    -e 'end timeout'; then
    echo 'Close the OD menu and quit the app before updating; installed files were preserved.' >&2
    exit 1
  fi
  for attempt in {1..30}; do
    /usr/bin/pgrep -f "^$target_app/Contents/MacOS/opendesk$" >/dev/null || break
    sleep 0.1
  done
  if /usr/bin/pgrep -f "^$target_app/Contents/MacOS/opendesk$" >/dev/null; then
    echo 'The app did not quit. Close it before updating; installed files were preserved.' >&2
    exit 1
  fi
  mv "$target_app" "$stage/previous.app"
fi
if ! mv "$stage/Open Desktop.app" "$target_app"; then
  [[ ! -e "$stage/previous.app" ]] || mv "$stage/previous.app" "$target_app"
  exit 1
fi
new_requirement="$(/usr/bin/codesign -d -r- "$target_app" 2>&1 | sed -n 's/^# //; s/^designated => //p')"
if [[ -n "$old_requirement" && "$old_requirement" != "$new_requirement" ]]; then
  # Reset only on an actual signing-identity migration (including old ad hoc builds).
  # Normal updates signed by the same development certificate preserve approval.
  for permission in ListenEvent Accessibility; do
    /usr/bin/tccutil reset "$permission" dev.mcintra.opendesk ||
      echo "Could not refresh $permission approval; renew it in System Settings." >&2
  done
fi
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister -f "$target_app"
ln -sfn "$target_app/Contents/MacOS/opendesk" "$HOME/.local/bin/opendesk"
/usr/bin/open "$target_app"
echo "Installed $target_app. Check input status in the OD menu."
