#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
cd "$root"
xcrun clang -fobjc-arc -mmacosx-version-min=26.0 -Wall -Wextra -Werror \
  test/macos-native.m crates/opendesk-macos/native/{app,input,commands,permissions}.m \
  -framework AppKit -framework CoreGraphics -framework Carbon -framework ServiceManagement \
  -o "$stage/native-tests"
"$stage/native-tests"
