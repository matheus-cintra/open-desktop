# open-desktop

Universal Control for Hyprland: push the cursor against a screen edge and it crosses to another Linux machine on the same LAN. Keyboard follows, the clipboard syncs, and a small bar on the edge shows the push progress and the arrival.

Requirements: Hyprland >= 0.56 on both machines, same LAN.

## Milestones

- M1: cursor and keyboard sharing, mDNS discovery, PIN pairing, edge crossing, keyboard capture with the compositor's own binds forwarded to the target, emergency release hotkey, control return, disconnect handling.
- M2: push resistance (the cursor holds at the edge and crosses only after you push past a threshold) and the on-screen bar (progress on the source, arrival on the target).
- M3: clipboard sync (text and images) between the two machines, always on, independent of the cursor.
- M4 (planned): file drag across the edge.

## Run

Build with `cargo build --release`, then on each machine:

```
systemctl --user enable --now opendesk    # or: opendesk daemon
opendesk pair <other-machine-name>          # type the PIN shown on the other machine
opendesk peer set <other-machine-name> --side left   # where the other screen sits
```

`opendesk status` shows the state and the paired peers. The config lives at `~/.config/opendesk/config.toml`; `edge_threshold_px`, `edge_cancel_px` and `bar_color` tune the gesture and the bar.

## Hyprland config

Add this line so the bar appears and disappears instantly instead of Hyprland's default layer fade:

```
layerrule = no_anim on, match:namespace ^opendesk-bar$
```

The daemon needs the UDP and TCP port (default 47820) open between the two machines. With ufw:

```
sudo ufw allow from <lan-subnet>/24 to any port 47820
```

Licensed under MIT or Apache-2.0, at your option.
