# macOS / Apple Silicon — experimental support

Version `0.3.0` targets Apple Silicon on macOS 26, paired with a
Hyprland Linux computers. Use one active display on each computer. This is an
experimental Mac build, signed with a development certificate and not notarized.

Download `open-desktop-macos-arm64.zip` and `SHA256SUMS-macos` from the
[v0.3.0 release](https://github.com/matheus-cintra/open-desktop/releases/tag/v0.3.0).
Verify the ZIP with `shasum -a 256 -c SHA256SUMS-macos`, unzip it, quit the old OD app
and its organization window, and move **Open Desktop.app** into `~/Applications`.
Keep a backup of your old app before replacement. Open the app and grant its permissions.
macOS may require explicit approval in Privacy & Security because it is not notarized.

## Build and install

On the Mac, install Rust 1.98 and Apple's Command Line Tools. In this checkout:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
bash scripts/build-macos.sh
# First build only, if requested (confirm on the Mac):
open scripts/trust-macos-signing.command
bash scripts/build-macos.sh
bash scripts/install-macos.sh
```

The build produces `dist/Open Desktop.app`, an ARM64 ZIP and its SHA-256 checksum.
The installer verifies the signature, installs in `~/Applications`, links
`~/.local/bin/opendesk`, and opens the app. Builds use a persistent local code-signing
certificate in `~/Library/Application Support/Open Desktop/signing`, outside the
checkout. Its private key is in a dedicated password-protected keychain; signing
unlocks it briefly and locks it afterward. Keep this directory to preserve identity.
The one-time trust helper authorizes that certificate only for code signing in the
current user's trust settings. This is not a Developer ID signature or notarized release.
The designated requirement pins both the certificate and bundle identifier.

Normal updates signed with the same certificate preserve TCC permissions. Moving
from older ad hoc builds to this identity requires one final authorization; only
an actual identity change triggers the installer's scoped permission reset.

While permission is pending, the app checks approval in a fresh, noninteractive
process every two seconds. If that process confirms both grants but the running
app is still pending, a helper waits for its exit and reopens it automatically.
A persisted guard allows at most one restart until input becomes ready, avoiding
restart loops. The probe never captures input, requests permission, or starts a
second daemon.

Open **OD** in the menu bar and grant Accessibility and Input Monitoring to
**Open Desktop**, and Local Network access if macOS requests it. Grant access to
the app, not to SSH or Terminal. The app stays available for pairing and diagnosis
while input permission is missing. Use **Iniciar ao entrar** to opt into launch at
login through `SMAppService`.

Pair by choosing a discovered computer and entering the PIN displayed on the
receiving computer. A received pairing request shows its PIN in the menu.
Use **Organizar computadores…** to arrange the shared map and apply it. CLI `setup`, `status`, `discover`,
`pair`, `peer`, `pause`, `resume`, `release`, `start`, `stop`, `restart`, `doctor` and
`logs` remain available. Public `update` is deliberately unavailable on macOS.

## Behavior

- Pointer and keyboard traverse the map directly from the physical origin.
- Physical activity on another computer takes control locally.
- Physical input is filtered at the HID event tap, before the window server
  processes local movement. Capture also hides and disassociates the source
  cursor, then restores it on release. Each captured motion also restores the
  local anchor position without generating input events, because background
  accessory apps cannot rely on cursor disassociation or activation requests.
- Ctrl and Super are swapped when targeting macOS; Command and Control are
  swapped when targeting Linux. This applies to terminal shortcuts too.
- Physical key positions use USB HID usages on the wire. The destination's active
  layout determines characters, accents and composition. Unknown usages are
  ignored; they are never interpreted as arbitrary native key codes.
- Ctrl+Alt+Esc on the source, or **Liberar controle** in the menu, releases control.
- Text and PNG images synchronize independently of which side controls the mouse.
  The existing clipboard size limit and echo suppression remain in the engine.
- Lock, sleep, inactive session, Secure Input or multiple active Mac displays
  suspend control. Permission loss and event-tap failure release captured input.
- Finder file dragging, login/password-screen control, Intel Macs and multiple
  monitors are outside this preview. Linux-to-Linux file dragging remains supported.

Protocol **4** keeps the old Hello frame shape for version rejection and adds
capability negotiation and physical-key messages. Run matching preview builds on
Linux and Mac. Protocol-2 and protocol-3 peers cannot control a protocol-4 peer. Existing
identities and pairing files retain their format.

## Architecture and state

`opendesk-platform` owns the shared command/event contract; the engine chooses a
Wayland or macOS backend at compile time. The Mac app keeps AppKit on the main
thread and Tokio on its runtime workers. A small Objective-C bridge provides
Core Graphics input, AppKit clipboard/menu/edge bars and ServiceManagement. No
kernel extension, root daemon or screen-recording access is required.

The event tap ignores events tagged by this backend to prevent feedback. Cursor
geometry uses logical display coordinates, including the Retina scale. Clipboard
bytes cross the native boundary directly, not as large JSON arrays. Input event
callbacks enqueue work; they do not wait for network round trips.

Mac configuration/identity/peers: `~/Library/Application Support/opendesk/`.
IPC: `~/Library/Application Support/Open Desktop/ipc.sock`.
Logs: `~/Library/Logs/Open Desktop/app.log`.

The native monitor conservatively combines console-session state, sleep/session
notifications, Secure Input and permissions. macOS lock detection includes the
session dictionary's `CGSSessionScreenIsLocked` key; treat lock behavior as
version-specific and revalidate on future macOS releases. No locked control is
advertised.

## Verification

```sh
cargo fmt --all --check
# Linux:
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
# Mac (do not build the Linux Wayland workspace member):
cargo +1.98.0 clippy -p opendesk -p opendesk-core -p opendesk-proto \
  -p opendesk-platform -p opendesk-macos --all-targets --locked -- -D warnings
cargo +1.98.0 test -p opendesk -p opendesk-core -p opendesk-proto \
  -p opendesk-platform -p opendesk-macos --locked
bash test/macos-native.sh
```

The native tests check keyboard mappings, logical coordinates and outward motion
at all four edges without capturing or injecting input. They do not replace the
physical matrix: all edges/both directions, local cursor staying still, accents,
modifier swapping, held/repeated keys, double-clicks, scrolling, text and screenshot
clipboard, emergency release, disconnect, lock/sleep and permission recovery.
Launching and querying the app over SSH proves process/network/IPC behavior only.
Record physical observations separately from builds and automated tests.
