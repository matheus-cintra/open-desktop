#!/bin/sh
# Open Desktop release bootstrap. Setup reads the terminal, not the script pipe.
set -eu
fail() { printf 'Open Desktop: %s\n' "$*" >&2; exit 1; }
[ "$(uname -s)" = Linux ] && [ "$(uname -m)" = x86_64 ] || fail 'Requer Linux x86_64.'
[ "$(id -u)" != 0 ] || fail 'Execute como seu usuário, sem sudo.'
[ -r /etc/os-release ] || fail 'Não foi possível identificar a distribuição.'
# This file is supplied by the OS, never by the download.
# shellcheck disable=SC1091
. /etc/os-release
case " ${ID:-} ${ID_LIKE:-} " in *' arch '*|*' cachyos '*) ;; *) fail 'Esta versão suporta Arch Linux e CachyOS.' ;; esac
for tool in curl tar sha256sum mktemp ldd bash; do
    command -v "$tool" >/dev/null 2>&1 || fail "Dependência ausente: $tool"
done
version=${1:-latest}
case "$version" in latest|v[0-9]*.[0-9]*.[0-9]*) ;; *) fail 'Use latest ou uma tag v0.1.0.' ;; esac
case "$version" in *[!a-zA-Z0-9._-]*) fail 'Tag inválida.' ;; esac
base=https://github.com/matheus-cintra/open-desktop/releases
if [ "$version" = latest ]; then base=$base/latest/download; else base=$base/download/$version; fi
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT HUP INT TERM
asset=open-desktop-linux-x86_64.tar.gz
printf 'Baixando Open Desktop (%s)…\n' "$version"
curl --proto '=https' --tlsv1.2 -fsSL --retry 3 "$base/$asset" -o "$stage/$asset"
curl --proto '=https' --tlsv1.2 -fsSL --retry 3 "$base/SHA256SUMS" -o "$stage/SHA256SUMS"
# Validate only the expected asset; never trust paths from a checksum manifest.
hash=$(awk -v file="$asset" '$2 == file {print $1}' "$stage/SHA256SUMS")
[ "${#hash}" = 64 ] || fail 'Checksum ausente ou ambíguo.'
case "$hash" in *[!a-fA-F0-9]*) fail 'Checksum inválido.' ;; esac
(cd "$stage"; printf '%s  %s\n' "$hash" "$asset" | sha256sum -c -)
# Extract only known regular payload paths from the release.
tar -xzf "$stage/$asset" -C "$stage" --no-same-owner --no-same-permissions opendesk
[ -f "$stage/opendesk" ] && [ ! -L "$stage/opendesk" ] || fail 'Binário inválido.'
chmod 755 "$stage/opendesk"
ldd "$stage/opendesk" > "$stage/ldd.txt" 2>&1 || { cat "$stage/ldd.txt" >&2; fail 'Dependências incompatíveis.'; }
if grep -q 'not found' "$stage/ldd.txt"; then cat "$stage/ldd.txt" >&2; fail 'Instale as dependências indicadas acima.'; fi
"$stage/opendesk" --version
"$stage/opendesk" install
if [ "${OPENDESK_NO_SETUP:-0}" != 1 ]; then
    if ( : </dev/tty ) 2>/dev/null; then
        printf '\nAbrindo configuração…\n'
        "$stage/opendesk" setup </dev/tty >/dev/tty 2>&1
    else
        printf '\nInstalação concluída. Execute em um terminal: ~/.local/bin/opendesk setup\n'
    fi
fi
case ":$PATH:" in *":$HOME/.local/bin:"*) ;; *) printf '\nAdicione ~/.local/bin ao PATH do seu shell para usar apenas opendesk.\n';; esac
