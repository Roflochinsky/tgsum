# Project selection and successful-analysis baselines

Implemented local selector and Project editor for `tgsum-hzm.3`, 2026-09-26.
The [Export only bundle](bundles.md) consumes this selector and verifies that
only selected messages are emitted. No agent is launched from the Project editor.

## Stored selection

Each `ProjectSource` has a `SourceSelection`:

- `enabled`: include the source in the next analysis; defaults to true.
- `only_changes`: omit unchanged messages relative to the last successful
  analysis; defaults to false. Without a baseline, all selected messages enter.
- `filter.topic_ids`: null means every topic, including future topics; an empty
  list means none. A list matches native thread IDs. Telegram topic-creation
  service events belong to their own message ID as the topic root.
- `filter.dates`: optional inclusive `from` and `through` calendar dates in
  `YYYY-MM-DD` form. Invalid or reversed bounds are rejected before publication.
- `filter.include_unknown_dates`: when date bounds exist, explicitly include
  messages whose date cannot be resolved in the requested basis. Defaults false.
- `filter.include_service`: include service events; defaults false.

`SourceDate` uses the calendar date written in the archive, including naive
timestamps, without converting through the computer's timezone. `Utc` requires
a known UTC instant from canonical timestamp metadata. A timestamp with an
explicit offset can therefore fall on different dates in the two modes. A naive
timestamp is unknown in UTC mode; it is never silently assumed to be UTC.
Without date bounds, undated messages remain eligible.

## Delta semantics

`scope::select_messages` consumes a current snapshot, the selected filter and
optionally a previous snapshot with the filter used by its successful analysis.
It matches scoped native message keys and canonical revision hashes. Neither
the highest message ID nor a Telegram export date range is the delta boundary.

An edit to an old message remains eligible if it is inside the current filter.
Expanding the filter includes messages that existed in the previous snapshot
but were outside the previous analysis: they count as newly selected. Narrowing
the filter does not mark the excluded messages as deleted.

The result distinguishes created, edited, explicitly deleted, unchanged and
missing records. Missing means an eligible baseline record is absent from the
entire current snapshot; absence in an incomplete archive does not prove
deletion. Missing records are counted but are not invented as current messages.
`unchanged` counts eligible records even when `only_changes` omits them from the
selected result. Excluded unknown dates and included unknown dates are reported
separately. Disabled sources return an empty selection.

Snapshots with different source namespaces cannot be compared. Snapshot-local
identities cannot be matched across different snapshots by this selector;
heuristic TXT matching remains a separate Archive Pack feature.

## Analysis ledger

The latest imported snapshot and the successful-analysis baseline are independent.
Refreshing an archive never marks its messages as analyzed.

`BeginAnalysis` freezes each enabled source's snapshot ID, selection and previous
baseline. It rejects overlapping runs, missing current snapshots and an empty
source selection. The current ledger records one active/latest run; an immutable
history of results and globally unique run allocation belong to later runner
and history work.

`FinishAnalysis(Succeeded)` advances each still-connected source's baseline to
the frozen inputs. Concurrent refreshes and filter changes do not change what
that run analyzed. `Failed` and `Cancelled` leave existing baselines untouched.
A late success cannot reattach a removed source or a source replaced with a
different namespace. Repeated finish calls and mismatched run IDs are rejected.

These are host operations. There is no UI button to claim a successful analysis.
The future runner must call success only after validating and durably storing
its result; the current ledger alone does not validate an AI output.

## Desktop flow

Projects → create/open a Project → connect chats from a local archive → select
chats/topics in the existing chooser → connect. The local account label forms
the namespace; it is not authenticated by the export. Different Telegram accounts
must use different labels.

Source cards persist enabled state, topic selection, inclusive date bounds,
date basis, unknown-date/service-event choices and changes-only mode. Preview
returns counts, topic names and observed coverage, without sending message text
to the WebView. Coverage remains a property of the observed archive, not a claim
that selecting fewer messages makes its history complete.

Refresh reads only the configured archive file and the selected conversation.
It atomically publishes a private snapshot before linking it with an optimistic
Project update. Cancel or a conflicting Project edit can leave an unreferenced
immutable snapshot; it cannot overwrite another edit or move the analysis
baseline. Retention will handle unreferenced snapshots separately. Removing a
source disconnects it; it does not delete the user's archive or retained files.

## Verification

- `core/tests/scope.rs`: topics and topic roots, timezone boundary differences,
  unknown dates, old edits, newly selected scope, missing versus deletion, and
  successful/failed/cancelled runs with concurrent refresh and source removal.
- `core/tests/project.rs`: v1 read/upgrade leaves old bytes unchanged, alongside
  persistence, corruption, namespace, optimistic-write and filesystem checks.
- `src-tauri/tests/commands.rs`: real backend IPC for import, topic/date preview,
  stale revision conflicts and failed analysis retaining its previous boundary.
- Actual Linux Tauri UI: create/connect the synthetic `sample-export.json`,
  select a topic/date, refresh, switch to UTC and include/exclude unknown dates.
  Restart checks persisted settings. This uses isolated application data and
  `GTK_CSD=1` for the native file dialog in the test desktop environment.

Core/IPC checks run under `bash scripts/check.sh`. Desktop checks do not certify
Windows/macOS, real messenger compatibility, account access, sanitizer coverage,
bundle isolation, agent execution or the complete onboarding flow.
