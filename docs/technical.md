# open-desktop

Universal Control for Hyprland: push the cursor against a screen edge and it crosses to another Linux machine on the same LAN. Keyboard follows, the clipboard syncs, files drag across, and a small bar on the edge shows the push progress and the arrival.

Tested target: Hyprland 0.56.2 Lua/UWSM on Arch and CachyOS, same trusted LAN. See the [user guide](../README.md).

## Milestones

- M1: cursor and keyboard sharing, mDNS discovery, PIN pairing, edge crossing, keyboard capture with the compositor's own binds forwarded to the target, emergency release hotkey, control return, disconnect handling.
- M2: push resistance (the cursor holds at the edge and crosses only after you push past a threshold) and the on-screen bar (progress on the source, arrival on the target).
- M3: clipboard sync (text and images) between the two machines, always on, independent of the cursor.
- M4: file drag across the edge — drag a file to the edge on one machine and it transfers and drops on the other.

## Install and run

This MVP targets CachyOS and Arch Linux with one output each and an active
Hyprland Lua session managed by UWSM. Both machines must run the same artifact:
network protocol 2 rejects older protocol versions. GUI setup is outside this MVP. Public CLI setup and release packaging are described in the [user guide](../README.md).

```sh
cargo build --release --locked
OPENDESK_BIN="$PWD/target/release/opendesk" ./scripts/install-user.sh
systemctl --user start opendesk.service
~/.local/bin/opendesk discover
~/.local/bin/opendesk pair <other-machine-name>
~/.local/bin/opendesk peer set <other-machine-name> --side all
~/.local/bin/opendesk status
```

Read the pairing PIN from `opendesk status` on the other machine. Each machine
creates its own identity; never copy identity or pairing tokens between hosts.
To install remotely, copy the binary, `scripts/install-user.sh`, and
`packaging/opendesk.service` into a separate staging directory over SSH, preserving
that directory structure. Compare `sha256sum` and check `ldd` on both hosts before
running the installer there with an absolute `OPENDESK_BIN`. Do not replace the
remote checkout or depend on Syncthing for installation.

The installer copies the executable into `~/.local/bin`, enables the user service,
and adds `conf/opendesk.lua` to `~/.config/hypr/hyprland.lua`. It preserves existing
modules and backs up replaced files under `~/.local/share/opendesk/backups`.
Reinstallation preserves configuration, identity and pairing. The Lua module
removes animation from the progress bar; no manual layer rule is needed.
Its non-consuming mouse-release hook is also required to finish cross-machine
file drops when Wayland's original drag holds the pointer grab.
Symlinked main Lua configs are rejected before mutation.

UWSM supplies the graphical environment through `graphical-session.target`, as
[documented by Hyprland](https://wiki.hypr.land/Useful-Utilities/Systemd-start/).
The service stores no temporary session identifiers. With an active session,
the installer reloads Hyprland and checks its errors through the user service
manager, including when invoked over SSH. A failed installation restores replaced
files. Start the service explicitly after installation; future graphical sessions
start the enabled service automatically.

## Four edges

`side` accepts `left`, `right`, `top`, `bottom`, or `all`; the positional form
`opendesk peer set NAME left` remains supported. `all` owns all four outer edges
on the supported single-output layout. Conflicts with another peer are rejected
without changing the previous configuration. Status shows `all`; each network
crossing still identifies one concrete direction.

Push against any edge until the progress bar completes. Arrival uses the opposite
edge and preserves proportional position. During remote control, only that entry
edge returns control, including file drag. Other edges cannot start a crossing.
`Ctrl+Alt+Esc` is the emergency release. Partial exposed edges in staggered
multi-monitor layouts are not supported by this MVP.

The config is `~/.config/opendesk/config.toml`; `edge_threshold_px`,
`edge_cancel_px`, and `bar_color` tune resistance and the bar.

## Diagnostics and network

```sh
systemctl --user status opendesk.service
journalctl --user -u opendesk.service -f
~/.local/bin/opendesk status
~/.local/bin/opendesk discover
systemd-run --user --wait --pipe --collect hyprctl configerrors
ldd ~/.local/bin/opendesk
```

Confirm the peer address is on the LAN. Allow TCP/UDP 47820 and mDNS UDP 5353
only on the appropriate LAN interface/subnet, preserving existing firewall rules:

```sh
sudo ufw allow in on <lan-interface> from <lan-subnet>/24 to any port 47820 proto tcp
sudo ufw allow in on <lan-interface> from <lan-subnet>/24 to any port 47820 proto udp
sudo ufw allow in on <lan-interface> from <lan-subnet>/24 to any port 5353 proto udp
```

After pairing, restarting both services must reconnect without another PIN.
Automated checks: `cargo fmt --check`, `cargo clippy --workspace --all-targets --
-D warnings`, `cargo test --workspace --locked`, `scripts/check-file-length.sh`,
`bash test/install-user-sandbox.sh`, and the nested tests `test/e2e_m1.py` and
`test/e2e_all_edges.py` (build the daemon and examples first). Physical mouse,
keyboard shortcuts, clipboard and file drag must also be checked in both directions.

## Stop, rollback and uninstall

Run `opendesk release` to restore local control, or use `Ctrl+Alt+Esc`.
`systemctl --user stop opendesk.service` stops the daemon;
`systemctl --user disable --now opendesk.service` also disables automatic startup.

`./scripts/install-user.sh uninstall` removes managed integration and restores
preexisting launcher/unit/module backups. It preserves `config.toml`, `peers.toml`,
and `identity.toml`. `./scripts/install-user.sh rollback-hyprland` restores the
recorded main-config backup; then reload Hyprland with the diagnostic service-manager
command above, replacing `configerrors` with `reload`. Backups remain available
for manual recovery. Remove only the firewall rules added for this app if no
longer needed. No checkout, configuration data or pairing store is deleted.

Licensed under MIT or Apache-2.0, at your option.

## Locked-screen control and recovery

The engine keeps `Unknown`, `Locked`, and `Unlocked` compositor state independent
of manual pause. A serial async worker queries Hyprland's `.socket.sock` directly:
`j/locked` every 500 ms locally / 100 ms during handoff, and `j/cursorpos` every
50 ms while receiving control on a locked screen. Scheduling has a 25 ms tick;
queries never block the engine, have a 200 ms timeout and a 4 KiB response limit.
The command formats are confirmed in [Hyprland 0.56.2 HyprCtl.cpp](https://raw.githubusercontent.com/hyprwm/Hyprland/v0.56.2/src/debug/HyprCtl.cpp).

Session transitions and layout changes advance a generation. Old results are
discarded. A locked return requires outward remote motion after arrival grace,
a subsequent cursor query confirming the recorded entry edge, and motion no
older than 200 ms. Inward motion cancels that intent. Logical xdg-output geometry
handles scaled displays and negative origins; right/bottom use the final valid
pixel. The proportional mapping and `EdgeEntered` session transition are shared
with ordinary returns. Locked screens ignore layer-surface edge events. Leaving
the session invalidates outstanding samples, preventing duplicate release.

Missing valid lock state (or required cursor state) for one second releases
control with existing `Disabled` messages. Locking the capturing source or a
file-drag destination uses the same cleanup, including keys, buttons, depressed
and latched modifiers, and drag cancellation. Toggle modifiers and keyboard group
are preserved. Locked targets deny file-drag grants; normal keyboard/mouse grants
remain possible. Unlocking resumes layer-surface detection without ending the
session or clearing manual pause. Wire protocol, pairing, and config are unchanged.

`test/e2e_locked.py` exercises actual Hyprland session locks in two nested
compositors and uses persistent virtual pointers to time the return. Its own
hyprlock child is unlocked with SIGUSR1 for the automated unlock transition;
this does **not** validate password entry or physical input. Run nested suites
sequentially. On hosts with multiple interfaces, isolate the fixture network:

```sh
unshare --user --map-current-user --keep-caps --net sh -c \
  'ip link set lo up; exec python3 test/e2e_locked.py'
```

Physical acceptance is separate: both directions, all four entry edges, wrong
edges rejected, return while still locked, re-entry/password/unlock continuation,
lock during control, no stuck inputs, and return within 250 ms after the arrival
guard. Record automated results separately from physical observations.
