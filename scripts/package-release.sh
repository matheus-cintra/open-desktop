#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if [[ -n "$(git status --porcelain)" ]]; then
  echo 'Commit all changes before packaging a release' >&2
  exit 1
fi
version="$(python3 -c 'import tomllib; print(tomllib.load(open("Cargo.toml", "rb"))["workspace"]["package"]["version"])')"
tag="${1:-v$version}"
[[ "$tag" == "v$version" ]] || { echo 'Tag must match Cargo.toml version' >&2; exit 1; }
binary="${OPENDESK_BIN:-$root/target/release/opendesk}"
[[ -x "$binary" ]] || { echo 'Run cargo build --release --locked first' >&2; exit 1; }
[[ "$("$binary" --version)" == "opendesk $version" ]] || { echo 'Binary version mismatch' >&2; exit 1; }
mkdir -p dist
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
install -m755 "$binary" "$stage/opendesk"
cp LICENSE-MIT LICENSE-APACHE "$stage/"
ldd "$binary" > "$stage/DEPENDENCIES.txt"
if grep -q 'not found' "$stage/DEPENDENCIES.txt"; then
  cat "$stage/DEPENDENCIES.txt" >&2
  exit 1
fi
printf 'version=%s\ncommit=%s\ntarget=x86_64-unknown-linux-gnu\nrust=%s\n' \
  "$version" "$(git rev-parse HEAD)" "$(rustc --version)" > "$stage/BUILD.txt"
# Stable asset name allows latest/download without parsing GitHub JSON.
# Bootstrap extracts this exact path (without ./).
tar --sort=name --mtime="@${SOURCE_DATE_EPOCH:-$(git show -s --format=%ct HEAD)}" \
  --owner=0 --group=0 --numeric-owner -czf dist/open-desktop-linux-x86_64.tar.gz -C "$stage" opendesk BUILD.txt DEPENDENCIES.txt LICENSE-MIT LICENSE-APACHE
cp install.sh dist/install.sh
(cd dist; sha256sum open-desktop-linux-x86_64.tar.gz install.sh > SHA256SUMS)
printf 'Release %s prepared in dist/\n' "$tag"
