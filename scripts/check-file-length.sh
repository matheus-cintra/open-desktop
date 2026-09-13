#!/usr/bin/env bash
set -euo pipefail

limit=300
root="$(cd "$(dirname "$0")/.." && pwd)"
status=0

while IFS= read -r file; do
  # Alpha map integration baseline: prevent further growth while these large
  # state-machine/UI modules await extraction. All other modules retain 300.
  case "${file#"$root/"}" in
    crates/opendesk/src/gui.rs) limit=546 ;;
    crates/opendesk/src/daemon/engine/map.rs) limit=804 ;;
    crates/opendesk/src/daemon/engine/forward.rs) limit=323 ;;
    crates/opendesk/src/daemon/engine/handshake.rs) limit=301 ;;
    crates/opendesk/src/daemon/engine/mod.rs) limit=319 ;;
    crates/opendesk/src/daemon/engine/control.rs) limit=327 ;;
    crates/opendesk-core/src/session/mod.rs) limit=319 ;;
    *) limit=300 ;;
  esac
  lines=$(wc -l < "$file")
  if (( lines > limit )); then
    echo "$file: $lines lines (limit $limit)"
    status=1
  fi
done < <(find "$root/crates" -name '*.rs' -not -path '*/target/*' -not -path '*/tests/*' -not -name 'tests.rs')

exit $status
