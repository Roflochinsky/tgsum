# Telegram refresh coordinator and desktop host

`core/src/bridge_schedule.rs` owns scheduling decisions and durable claims.
`core/src/bridge_refresh.rs` runs an injected `RefreshDriver`, observes correlated
events and publishes validated results. `src-tauri/src/telegram_refresh.rs`
connects the coordinator to a one-second application timer and Tauri commands.
The source card exposes cadence, refresh, cancellation and interrupted-attempt
recovery in `ui/telegram-refresh.js`.

The production host currently binds `UnavailableDriver` and an unknown/inactive
session observation. It performs no Telegram acquisition. Non-manual cadence
options and the automatic-refresh button are unavailable until a qualified
OS driver/session observer is registered. UI focus does not prove that the OS
session is unlocked. The opt-in debug desktop harness injects a synthetic driver
and clock; this binding is absent from release builds.

## Scheduling and persistence

Project schema v9 stores `telegram_refresh` per selected Telegram JSON source.
Old manifests open with an empty map and manual default without rewriting them.
Opt-in modes are `on_start`, `every_six_hours` and `daily`. Scheduled attempts
require an active unlocked session, an available global lease and expiration of
client delays/backoff. Explicit manual refresh bypasses cadence, but still
honours locks, unresolved attempts and retry delay. One timer tick dispatches at
most one due source; sleep/restart does not replay a queue of missed timer events.

`TelegramExportLease` holds one OS file lock beneath the Project root across
projects and processes. A claim pins the complete reviewed Project revision and
is persisted before `driver.start`. A concurrent edit prevents dispatch or final
publication. After a crash the OS lock disappears, but an unresolved checkpoint
in **any** Project still prevents another source from starting. Removing or
replacing that source cannot erase the checkpoint; deleting acquisition settings
retains it. An unreadable Project also prevents claiming a new export because
its previous state cannot be established.

## Driver and completion contract

The driver must implement bounded `start`, `poll` and `cancel` calls. Only `poll`
observes events; it never retries a mutating action. Every event echoes the fresh
random run ID, selected Project/source, namespace and destination settings.
Stale or mismatched events cannot publish. Completion requires an earlier
`Exporting` observation and an actual archive path reported by the driver, so a
new nested `ChatExport_*` directory is supported without guessing its name.
These checks establish request correlation; the native driver must separately
qualify the truth of account/chat and completion observations.

The archive is opened relative to a retained destination-directory handle,
rejecting links, reparse points, traversal and non-regular files. Two bounded
streaming reads verify stable bytes before parsing. The importer validates the
full JSON and selected native conversation ID. The resulting immutable snapshot,
actual archive reference and resolved checkpoint are published in one Project
revision. Failures preserve the previous source snapshot and analysis baseline;
a publication conflict can leave an unreferenced immutable snapshot.

Cancellation has a per-run atomic flag that can interrupt file staging/import.
A cancel action is delivered to the driver at most once; its delivery alone is
not a terminal client outcome. An observed terminal cancellation/failure clears
the claim with backoff. Completion after a cancellation request never imports.
Timeout, a lost active session or an ambiguous driver error leaves the claim
unresolved for review. The application host currently bounds a run to six hours;
this is a product timeout, not a Telegram platform limit.

## Recovery and UI

The recovery command requires the user to confirm that the official client is
no longer exporting and pins the displayed Project revision/checkpoint. It
clears the unresolved attempt with a 15-minute backoff, without asserting that
an archive was imported. A leftover archive goes through normal assisted import.
Recovery is also available when no automatic driver is registered.

Tauri cancellation requires the exact run ID; a delayed request cannot stop a
later export of the same source. Status is scoped to the selected Project/source.
Cadence changes refuse an active run in that Project. Stale UI revisions require
reload. A completed refresh reports created/edited/missing counts and does not
start AI analysis or advance its baseline.

## Verification scope

- `core/tests/bridge_schedule.rs`: cadence, restart, process lease, reviewed
  recovery, delays, backwards clock and schema migration.
- `core/tests/bridge_refresh.rs`: simulated driver, real archive/Project storage,
  new output directories, delta, cancellation, late/wrong events, invalid files,
  links, concurrent edits, cross-Project interrupted attempts and timer wake-up.
- `src-tauri/tests/commands/telegram_refresh.rs`: actual IPC and timer integration
  with an injected driver/clock, cadence, cancellation identity and recovery.
- `scripts/desktop-e2e.py`: actual WebView controls with an explicitly enabled
  debug fixture driver. No real Telegram application/account is touched.

Installed-client observations `tgsum-t8t.23/.24/.25` and OS drivers
`tgsum-i1w.2/.3/.4` remain separate work. Synthetic tests qualify coordinator,
IPC and UI behaviour, not automatic export from Telegram Desktop. Registry
`telegram_desktop_ui` stays `research` with no enabled operations.
