# Project storage

Implemented backend contract for `tgsum-hzm.1`, extended by the Project scope
editor in `tgsum-hzm.3`, 2026-09-26. The Projects screen creates, opens, renames
and connects selected Telegram archive conversations. See [scope and analysis
baselines](scope.md) for its filters and refresh behavior, and [bundles](bundles.md)
for local Privacy Review and Export only. Full onboarding and
analysis/result screens remain separate slices.

`ProjectStore` creates, lists, opens and updates project configuration. Tauri
keeps it under its application-local data directory, in `projects/`. Tests
inject temporary stores. Creating a project does not open any messenger or
copy its archive. A source contains a locally configured account namespace,
one conversation scope, an optional absolute archive path and an optional
snapshot reference. Attachment copying belongs to the packaging layer.

```text
projects/project-<random>/
  revisions/00000000000000000000.json
  revisions/00000000000000000001.json
  snapshots/<snapshot-id>.json
```

## Persistence and recovery

Manifests have `schema_version: 2`, adding source selections and an analysis
baseline ledger. Version 1 opens in memory with default selections and no
analysis baseline. Reading does not modify the original file; the next update
publishes a separate v2 revision. Unknown versions are rejected before
interpreting their fields and are never rewritten implicitly. Tests verify
that the v1 bytes remain unchanged after both reading and upgrading.

Each update supplies `expected_revision`. Publication uses a flushed, synced
temporary file and `persist_noclobber` for the next immutable revision. Two
editors cannot replace each other's committed revision; the loser receives
`WouldBlock` (IPC `kind: conflict`) and must reload. Interrupted staging files
are ignored. The highest committed revision is authoritative: corruption is
reported without silently rolling back to older settings. An operator can
inspect/restore a known revision; automatic recovery UI is not implemented.

`list` returns individual `unavailable` entries for damaged projects so that
other projects remain usable. Manifests are limited to 1 MiB. Invalid IDs,
duplicate source IDs, relative archive paths and zero token limits are rejected.
Project, revision and snapshot directory symlink substitutions are rejected at
inspection. The store is not a defense against concurrent writes by another
process running as the same user; filesystem race resistance is not claimed.

On Unix, newly created project directories are `0700`, manifests `0600`.
Other systems rely on the user's application-data directory access controls.
No encryption at rest or power-loss directory durability is promised.

## Source and snapshot handling

`ProjectSource::availability` checks the selected file's metadata. A moved or
missing archive reports `file_missing`; the Project and stored snapshots still
open. Updating a source or recording a snapshot verifies that the snapshot is
in this Project and matches its platform/account/conversation scope. A rename
or settings edit never loads the potentially large snapshot payload.

Removing a source removes its manifest reference. It does not delete the
external archive or snapshots; retention and deletion are a separate slice.
Historical manifests retain previous source paths until that retention feature.
The store contains private configuration and raw snapshots; agents must receive
a separately prepared, sanitized context bundle.

## Desktop commands

| Command | Arguments / result |
| --- | --- |
| `create_project` | `name` → Project |
| `list_projects` | → ready/unavailable entries |
| `open_project` | `projectId` → Project |
| `update_project` | `projectId`, `expectedRevision`, `change` → Project |
| `project_source_status` | `projectId`, `sourceId` → availability |
| `preview_project_source` | `projectId`, `sourceId` → selection counts, topics, coverage, baseline ID |
| `refresh_project_source` | `projectId`, `sourceId`, `expectedRevision` → Project with new snapshot |
| `prepare_project_bundle` | `projectId`, `expectedRevision`, `options` → local Review |
| `export_project_bundle` | `projectId`, `bundleId`, `expectedRevision`, `outDir` → exported directory/files |

Command arguments use Tauri camelCase. Project fields use the Rust snake_case
schema. Changes use `{kind, value}`: `rename`, `source` (upsert), `remove_source`,
`settings`, `record_snapshot`, `selection`, `begin_analysis`, `finish_analysis`.
Errors are `failed`, `conflict` or `cancelled`.
The default settings select `export_only`, `summary`, and the `secrets` preset;
these are configuration references, not a claim that a runner or sanitizer has
already executed.

Verified with synthetic core lifecycle/concurrency/corruption/migration tests,
Tauri mock IPC, and the actual Linux Tauri Project editor with a synthetic
Telegram fixture. No real messenger account, client export or credentials are
used. Windows/macOS desktop qualification remains a separate QA task.
