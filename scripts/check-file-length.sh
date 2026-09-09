#!/usr/bin/env bash
set -euo pipefail

limit=300
root="$(cd "$(dirname "$0")/.." && pwd)"
status=0

while IFS= read -r file; do
  lines=$(wc -l < "$file")
  if (( lines > limit )); then
    echo "$file: $lines lines (limit $limit)"
    status=1
  fi
done < <(find "$root/crates" -name '*.rs' -not -path '*/target/*' -not -path '*/tests/*' -not -name 'tests.rs')

exit $status
