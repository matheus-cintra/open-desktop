#!/bin/bash
set -euo pipefail
[[ "$(uname -s)" == Darwin && "$(uname -m)" == arm64 ]] || { echo 'Build on Apple Silicon macOS'; exit 1; }
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
export MACOSX_DEPLOYMENT_TARGET=26.0
cargo +1.98.0 build -p opendesk --release --locked
app="$root/dist/Open Desktop.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
install -m755 target/release/opendesk "$app/Contents/MacOS/opendesk"
version="$(target/release/opendesk --version | awk '{print $2}')"
bundle_version="${version%%-*}"
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>dev.mcintra.opendesk</string>
<key>CFBundleName</key><string>Open Desktop</string>
<key>CFBundleDisplayName</key><string>Open Desktop</string>
<key>CFBundleExecutable</key><string>opendesk</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$bundle_version</string>
<key>CFBundleVersion</key><string>$bundle_version</string>
<key>LSMinimumSystemVersion</key><string>26.0</string>
<key>LSUIElement</key><true/>
<key>NSHighResolutionCapable</key><true/>
<key>NSLocalNetworkUsageDescription</key><string>Conectar aos seus computadores para compartilhar mouse, teclado e clipboard.</string>
<key>NSBonjourServices</key><array><string>_opendesk._tcp</string></array>
</dict></plist>
PLIST
cp LICENSE-MIT LICENSE-APACHE "$app/Contents/Resources/"
printf 'target=aarch64-apple-darwin\nprotocol=4\nsigning=local-certificate\nrust=%s\n' "$(rustc +1.98.0 --version)" > "$app/Contents/Resources/BUILD.txt"
printf 'version=%s\ncommit=%s\n' "$version" "${OPENDESK_BUILD_COMMIT:-$(git rev-parse HEAD)}" >> "$app/Contents/Resources/BUILD.txt"
/usr/bin/plutil -lint "$app/Contents/Info.plist"
source "$root/scripts/macos-signing.sh"
trap '/usr/bin/security lock-keychain "$keychain"' EXIT
/usr/bin/codesign --force --sign "$signing_hash" --keychain "$keychain" \
  --requirements "=$signing_requirement" --identifier dev.mcintra.opendesk "$app"
/usr/bin/codesign --verify --strict --verbose=2 "$app"
/usr/bin/ditto -c -k --keepParent "$app" dist/open-desktop-macos-arm64.zip
(cd dist && shasum -a 256 open-desktop-macos-arm64.zip > SHA256SUMS-macos)
echo "Built $app (local preview; not notarized)"
