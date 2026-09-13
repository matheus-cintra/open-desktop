#!/bin/bash
# Sourced by build-macos.sh. Keep the development identity outside the checkout.
set -euo pipefail
signing_dir="$HOME/Library/Application Support/Open Desktop/signing"
keychain="$signing_dir/development.keychain-db"
cert="$signing_dir/development.der"
password_file="$signing_dir/keychain-password"
mkdir -p "$signing_dir"
chmod 700 "$signing_dir"
if [[ ! -f "$cert" ]]; then
  if [[ -e "$keychain" || -e "$password_file" ]]; then
    echo 'Incomplete signing identity; preserve the signing directory and repair it before building.' >&2
    exit 1
  fi
  (
    umask 077
    stage="$(mktemp -d "$signing_dir/setup.XXXXXX")"
    trap 'rm -rf "$stage"' EXIT
    /usr/bin/openssl rand -base64 32 > "$password_file"
    signing_password="$(cat "$password_file")"
    cat > "$stage/cert.cnf" <<'CONFIG'
[req]
distinguished_name=dn
x509_extensions=extensions
prompt=no
[dn]
CN=Open Desktop Local Development
[extensions]
basicConstraints=critical,CA:false
keyUsage=critical,digitalSignature
extendedKeyUsage=codeSigning
CONFIG
    /usr/bin/openssl req -new -newkey rsa:3072 -nodes -x509 -days 3650 \
      -config "$stage/cert.cnf" -keyout "$stage/key.pem" -out "$stage/cert.pem" 2>/dev/null
    /usr/bin/openssl pkcs12 -export -inkey "$stage/key.pem" -in "$stage/cert.pem" \
      -out "$stage/identity.p12" -passout "file:$password_file"
    /usr/bin/security create-keychain -p "$signing_password" "$keychain"
    /usr/bin/security set-keychain-settings -lut 300 "$keychain"
    /usr/bin/security unlock-keychain -p "$signing_password" "$keychain"
    /usr/bin/security import "$stage/identity.p12" -k "$keychain" -P "$signing_password" \
      -T /usr/bin/codesign >/dev/null
    /usr/bin/security set-key-partition-list -S apple-tool: -s -k "$signing_password" "$keychain" >/dev/null
    /usr/bin/openssl x509 -in "$stage/cert.pem" -outform DER -out "$cert"
  )
fi
[[ -f "$password_file" && -f "$keychain" ]] || { echo 'Signing keychain is missing; refusing to change identity.' >&2; exit 1; }
# codesign also needs this keychain in the search list to resolve its chain.
# Preserve all existing entries, including login and any developer keychains.
/usr/bin/python3 - "$keychain" <<'PYCHAIN'
import shlex, subprocess, sys
paths = shlex.split(subprocess.check_output(["security", "list-keychains", "-d", "user"], text=True))
if sys.argv[1] not in paths:
    subprocess.run(["security", "list-keychains", "-d", "user", "-s", *paths, sys.argv[1]], check=True)
PYCHAIN
signing_password="$(cat "$password_file")"
/usr/bin/security unlock-keychain -p "$signing_password" "$keychain"
unset signing_password
signing_hash="$(/usr/bin/shasum -a 1 "$cert" | awk '{print toupper($1)}')"
if ! /usr/bin/security find-identity -v -p codesigning "$keychain" | /usr/bin/grep -q "$signing_hash"; then
  /usr/bin/security lock-keychain "$keychain"
  echo 'Authorize the local signing certificate once, on the Mac:' >&2
  echo '  open scripts/trust-macos-signing.command' >&2
  exit 1
fi
# Bind authorization to the actual signing certificate, never to just the bundle ID.
signing_requirement="designated => identifier \"dev.mcintra.opendesk\" and certificate leaf = H\"$signing_hash\""
