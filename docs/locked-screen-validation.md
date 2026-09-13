# Locked-screen return validation — 2026-09-13

## Implementation and installed artifact

Workspace: `/home/matheus/dev/pessoal/open-desktop`, branch `feat/m0-scaffold`.
The physical acceptance below was performed before release preparation, on base
commit `f09e0a3f2569160bf13d2fa6d22a2996369ea772` plus the locked-screen correction.
At that point, the correction had not been committed or published. It is now
included in v0.1.2; the release workflow records its exact commit in `BUILD.txt`.

Pre-release validation build (reported version 0.1.1), built with
`cargo build --release --locked`. The exact same executable was
installed on CachyOS and Arch Linux using `scripts/install-user.sh`:

```text
SHA256 3e0481d9d595ca86d07d6c7e52085236bc1b0abc666e98de6b20bb5780839ad9
```

Both user services are active and reconnected after their restarts. `ldd` found
all dependencies on both hosts. SHA256 verification confirms `config.toml`,
`peers.toml`, and `identity.toml` stayed byte-identical on each machine.

## Automated results

| Check | Result |
| --- | --- |
| `cargo fmt --check` | Passed |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Passed |
| `cargo test --workspace --locked` | 162 passed, 0 failed, 1 ignored |
| `scripts/check-file-length.sh` | Passed, maximum 300 lines per production Rust file |
| `git diff --check` | Passed |
| `bash test/install-user-sandbox.sh` | Passed |
| `test/e2e_all_edges.py` | 88/88 passed |
| `test/e2e_locked.py` | 66/66 passed |
| `test/e2e_m1.py` | 38/38 passed |

The ignored test is the existing real desktop notification test, which requires
a notification daemon. Fifteen new engine/socket tests cover the four logical
edges, negative origins, scaled logical geometry, proportional position, final
right/bottom pixels, arrival grace, inward/stale motion, generation rejection,
single-query scheduling, 500/100/50 ms polling, pause, cleanup and socket failures.

The locked integration suite uses real Hyprland session locks and direct socket
queries in two nested compositors, with persistent virtual pointers. Eight
return measurements, including the injector's 20 ms settling delay:

| Source | Exit edge | Return latency |
| --- | --- | --- |
| alpha | left | 36.2 ms |
| alpha | right | 52.1 ms |
| alpha | top | 41.3 ms |
| alpha | bottom | 51.8 ms |
| beta | left | 20.5 ms |
| beta | right | 57.1 ms |
| beta | top | 25.9 ms |
| beta | bottom | 46.7 ms |

Every destination stayed locked after return. Other edges and stationary entry
positions did not return control. Unlocking preserved the active session;
locking a destination mid-session preserved input and allowed edge return;
locking the captured source released both machines. A return can arm the existing
local edge-push resistance; the test separately confirms inward local motion
clears it. It does not count that local pushing state as remote capture.

The ordinary suites cover proportional crossings, clipboard, file drag in both
directions, emergency release (including held drag), UDP/TCP behavior and release
when the peer process dies. Installation separately proved real-peer reconnection.

An initial ordinary run passed 78/88 on the host network. Logs showed nested
peers selecting different local interfaces and rejecting UDP as an unknown
source. The untouched base binary passed 88/88 in a comparison run. Final nested
runs isolated networking to loopback, eliminating interface selection from this
regression test; production network behavior was not changed. Early versions of
the new fixture also included virtual-device startup in the latency and started
alpha's daemon before beta's window resized alpha. The final fixture uses
persistent devices and starts both compositors before the daemons.

Reproduce each suite sequentially after building the debug daemon and examples:

```sh
cargo build --workspace --examples --bin opendesk --locked
unshare --user --map-current-user --keep-caps --net sh -c \
  'ip link set lo up; exec python3 test/e2e_all_edges.py'
unshare --user --map-current-user --keep-caps --net sh -c \
  'ip link set lo up; exec python3 test/e2e_locked.py'
unshare --user --map-current-user --keep-caps --net sh -c \
  'ip link set lo up; exec python3 test/e2e_m1.py'
```

Automated run logs are in `/tmp/opendesk-lock-*.log` on the development host.
These temporary files are supplementary evidence, not required to run the tests.

## Physical acceptance

Accepted by the user after installation: “Tudo passou nos dois sentidos e nas
quatro bordas.” This answered the requested physical matrix: CachyOS → Arch Linux
and Arch Linux → CachyOS, four entry edges, wrong-edge rejection, return without
unlocking or emergency hotkey, password entry and continued input, lock during
control, and no stuck keys/buttons.

This is user-confirmed physical evidence, separate from automated test results.
Temporary read-only observation also recorded actual controlled/idle transitions
while the destination remained locked. It recorded only daemon state, lock state,
enabled/connected status and timestamps, never key events or passwords. The
observers were stopped after acceptance.

The 250 ms bound was measured in nested integration (20.5–57.1 ms), not with
physical hardware timing. The user reported the physical matrix passed; exact
physical latency remains unmeasured.

## v0.1.2 publication

The release changes the package version to 0.1.2 without changing the physically
accepted control behavior. GitHub Actions builds the public artifact on Ubuntu
24.04 with Rust 1.98.0. Its checksum differs from the pre-release local validation
build above; use the published `SHA256SUMS` and `BUILD.txt` for release provenance.
