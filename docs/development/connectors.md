# Connector contracts

Implemented backend seam for `tgsum-hzm.4`, 2026-09-26. Only Telegram JSON is
a production-format normalizer in this slice. The second normalizer and
acquisition driver are synthetic test implementations, not platform support.

## Normalize, then publish

`ArchiveImporter` exposes an implementation descriptor and normalizes an input
`Read` into one selected `ConversationObservation`. Its callback can stage the
selected chat before the remainder of a large full archive is read. The
normalizer must validate the full source, including data after the selected
chat, and propagate errors. It receives no output directory or credential API.

`SnapshotStore::import` owns publication. It validates platform, selected source
and record identities, computes canonical metadata, stages one observation and
publishes only after successful normalization and EOF. Zero or multiple
observations, wrong scope, ignored callback errors, unread input and late parse
errors abort publication. EOF does not prove semantic validity: buffered
normalizers remain responsible for validating every parsed byte.

The store writes to a private temporary file, syncs it and publishes without
replacement. Telegram still streams unselected chats one at a time and retains
only the selected snapshot. `import_telegram` remains a compatibility wrapper
around the same path and `TelegramJson` normalizer.

`publish_observation` accepts an already acquired canonical observation. API
or bot code can use it without inventing an intermediate Telegram file. It
does not fetch anything, save tokens or commit an API checkpoint. Atomic
checkpoint/event delivery belongs to the later business-connector runtime.

## Source facts and derived metadata

An observation supplies conversation metadata/coverage and message source facts:
canonical fields, identity quality and explicit deletion state. The store
computes provenance, timestamps and revisions, replacing any supplied derived
metadata. Capabilities never imply complete coverage. An adapter that declares
no stable IDs cannot emit `native` identity. Snapshot-local IDs can be used
within that observation; cross-snapshot diff requires a separate matching
strategy and currently returns an error.

Diff now returns a separate `deleted` list for explicit tombstones. Absence
still returns `missing`; an unchanged tombstone is unchanged on a repeat run.
Telegram archive import only emits `present` and cannot infer deletions.

## Acquisition and Bridge

`SourceAcquisition` is an optional start/poll/cancel driver interface. It emits
events correlated to an explicit `ExportRequest`; the host controls scheduling
and feeds them to `ExportJob`. A driver must observe completion, rather than
treat file existence or a quiet watcher as success. This slice provides no
real network, OS automation, credential store or unattended client driver.

`ExportJob::for_importer` binds a request to a descriptor; `apply_with_importer`
rejects a different normalizer before opening the artifact. Existing `new` and
`apply` methods retain their Telegram behavior. Event correlation, terminal
states and no-publication-on-failure rules apply to the synthetic second
connector too. Local archives/backups can be passed directly to an importer
without any acquisition driver.

Descriptors identify implementation revision/format, archive/local/OAuth/bot/
official-client class, implemented history/refresh/identity/attachment behavior,
and credential kind. They contain no credential value. The file core has no
network/authentication runtime dependency. Descriptors are distinct from the
dated policy/qualification registry: two acquisition profiles can share a
normalizer. That registry's loader and release checks are a separate task.

## Verification

Public contract tests use a tiny synthetic tab-record format and simulated
acquisition: import/reload/diff, repeated text, exact IDs, wrong namespace,
duplicate records, late/ignored failures, unfinished input, tombstones,
snapshot-local identity, bound normalizer and late completion. Existing Telegram
parser, snapshot, migration and Bridge tests use the refactored path unchanged.
No account or actual messenger client is involved.
