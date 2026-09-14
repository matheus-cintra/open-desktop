# macOS updater verification — 2026-09-13

Implemented on `main`, base `a4775df`, with uncommitted updater changes. No release,
tag, push or modification of public v0.3.0 assets was performed. Both checkouts
started at the expected base. Mac: Apple Silicon, macOS 26.6.2 (25G83).

## Automated results

- Linux: formatting, file length checks, workspace tests **183 passed / 1 ignored**,
  workspace Clippy with warnings denied, shell syntax and diff whitespace checks.
- Mac: formatting, file length checks, tests for opendesk/core/proto/platform/macos
  **117 passed**, Clippy for the same packages with warnings denied, release build
  and existing native mapping/geometry/edge tests.
- Installer suite: **23 passed on both hosts**. Actual installer functions and
  isolated filesystem transactions; mocked OS/network boundaries. Covers unavailable
  download, checksum missing/duplicate/invalid/mismatched, invalid ZIP, traversal,
  symlink entries, concrete latest resolution, rejected preflight, exclusive lock,
  stubborn process, scoped GUI signal, replacement failure, startup failure and
  restoration of app/data/CLI, backup failure, failed recovery preserving backups,
  actual subprocess SIGTERM rollback and SIGKILL recovery on the next invocation,
  IPC absence and IPC belonging to a different process.
- Native updater suite: **6 passed**. Real `ditto`, `codesign`, certificate extraction,
  architecture/version checks using disposable copies of the installed signed app.
  Rejects ad hoc re-signing, altered identifier, incorrect requested version and
  signed-resource tampering. Controlled initialization failure also restored a real
  signed bundle, fixture identity and CLI link in an isolated home. Only lifecycle
  and user preference operations were mocked in that rollback case, so the live
  user's processes/settings were unaffected.

Commands and outcomes: [checks.txt](checks.txt).

## Live Mac installation

Initial installed app: `0.3.0-alpha.1`, input `ready`, two connected peers. Installed
the checked public v0.3.0 to establish the baseline, then installed the local updater
build signed by the existing certificate.

Exercised all three CLI forms, with the organization window open:

1. `opendesk update v0.3.0`: public release installed and GUI process reopened.
2. Reinstalled the updater build, then `opendesk update latest`: resolved to
   `v0.3.0` before downloading both assets and completed successfully.
3. Built/installed the final updater revision, then `opendesk update`: resolved to
   `v0.3.0` and completed successfully. Reinstalled that final local build afterward,
   leaving the updater available on the Mac.

The explicit update replaced app/GUI PIDs `62643/62685` with `62731/62735`.
[Before](before-cli.json) and [after](after-cli.json) snapshots show identical hashes
for every persisted file and the map report, and the same peer identity and paired
computers. Only hashes, public identifiers and status are recorded, never keys/PINs.

[Final state](final.json): input **ready**, both peers connected, app and GUI process
running. Identity, config, peers and persisted map hashes still match the pre-update
snapshot. `control-clock.json` subsequently advanced with normal control activity;
the full IPC map report hash also includes live state, so it is not a persisted-map
integrity check. The persisted `map.json` hash is unchanged.

[Installed proof](installed-proof.txt): installed binary SHA-256 equals the final
local build (`32318e4eca993360f48d6554e440fe609ff7ee615e54984eff5ab787ea693493`),
strict signature verification passed, original designated requirement still pins
certificate `0f939256b2da742cb81ad8f035a0e668c6526281`, embedded updater rejects an
invalid tag with exit 1, and no unfinished transaction journal remains. The build
reports version 0.3.0 but contains the local changes; this is not a new release.

Live command excerpts: [explicit](mac-cli-explicit.txt), [latest](mac-cli-latest.txt),
[final default update and reinstall](mac-final-cli.txt). Latest retained backup:
`~/Library/Application Support/Open Desktop/backups/update.ei1_by_1` on the Mac.
Earlier backups remain available, including the initial alpha and updater builds.

## Physical validation boundary

SSH proved process replacement/reopening, signature identity, IPC ownership/health,
persisted state integrity, connectivity and final input status. During the run,
input temporarily reported `session-unavailable`, then returned to `ready`.
A direct permission probe invoked from SSH returned 0; that invocation has SSH's
execution context and is not substituted for the app's permission/physical state.

The user was asked to confirm visible GUI reopening, retained permissions and
mouse/keyboard traversal between computers; no physical confirmation was received
in this run. Visual appearance, unsaved-editor behavior and real input traversal
remain unconfirmed. The login registration was not changed by the installer; a
logout/login cycle and direct SMAppService status check were not performed.

Public v0.3.0 predates the updater. Installing it removes the new CLI implementation;
this is why the final local updater build was reinstalled. Usage and recovery are
in [docs/macos.md](../../macos.md).
