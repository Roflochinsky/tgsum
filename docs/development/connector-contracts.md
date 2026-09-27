# Reusable offline connector contracts

The shared suite lives in
[`core/tests/support/connector_contract.rs`](../../core/tests/support/connector_contract.rs).
[`core/tests/connector_contract.rs`](../../core/tests/connector_contract.rs)
runs it against the production `TelegramJson` importer and an independent
synthetic JSON Lines importer. The latter exists only in the test binary and
is not registered as a product connector.

```sh
cargo test --locked -p tgsum-core --test connector_contract
```

## Adding an importer

Import the support module from the new integration test and encode the common
corpus in the source's actual fixture format. Pass the real `ArchiveImporter`
to `archive_contract` and `acquisition_contract`; keep the assertions shared.
Do not translate all source fixtures into canonical objects before testing the
production parser. No accounts or installed messenger clients are required.

| Corpus input | Required observation |
| --- | --- |
| `before` | Two distinct IDs `9007199254740993` and `9007199254740994`, identical UTF-8 text; preserve both |
| First timestamp | `1970-01-01T03:00:00+03:00` → UTC second 0; retain the raw timestamp |
| Second timestamp | `2026-06-18T10:00:00` → timezone unknown, no invented UTC |
| `after` | First ID edited, second absent, ID `3` created |
| `malformed` | A source-representation error; prefer one after selected records |
| `duplicate` | Repeated native message ID within the selected conversation |
| `coverage` | The actual expected coverage, independent of `history: true` |
| Optional `tombstone` | Only first ID explicitly deleted, second absent |

The corpus deliberately uses IDs beyond JavaScript's exact integer range and
identical message text. It exercises identity independently of content hashing.
For a format without stable IDs, use `snapshot_local_contract` with the same
two repeated texts, declare `stable_message_ids: false` and emit
`IdentityQuality::SnapshotLocal`. It checks distinct records within the snapshot
and refuses cross-snapshot matching even when the same input is imported twice.
Do not invent stable IDs from positions or content just to pass the native-ID
suite. A synthetic position-ID adapter exercises this branch and the same
acquisition lifecycle; native-specific edit/deletion assertions do not apply.

## Assertions

`archive_contract` uses the real `SnapshotStore` to verify:

- Native keys include the configured account and conversation namespace;
  another account cannot be diffed into this one, and wrong-platform input
  cannot publish.
- Raw timestamps and explicit/unknown timezone semantics survive normalization
  and storage; identical text with distinct IDs remains distinct.
- Reload and repeat import preserve the canonical content; a repeat has only
  unchanged messages. Partial/unknown coverage remains partial/unknown.
- Created, edited and missing are separate. Missing does not become deletion.
  Only a supplied native tombstone is tested as explicit deletion.
- Snapshot IDs are immutable. Malformed input, duplicate IDs, interrupted reads
  and an I/O failure at the EOF probe publish no checkpoint and preserve the
  prior snapshot. An aborted store publication can be retried.

`acquisition_contract` drives the real `ExportJob` through a scripted
`SourceAcquisition`. It checks that a readable, complete file is insufficient
before a matching completion event; stale run/account/conversation/path events
cannot publish; cancellation and failed normalization are terminal; a new
attempt can succeed; duplicate completion cannot publish again.

Here **checkpoint means the published immutable snapshot**. Remote API cursor
acknowledgement, pagination, OAuth expiry and provider-specific retries require
their own adapter tests once implemented. This generic suite cannot qualify
those unimplemented interfaces. It also cannot qualify actual GUI automation,
account access or a messenger's completion signal: its acquisition driver is
explicitly synthetic.

The companion registry boundary test verifies that a passing synthetic adapter
plus edited inventory flags cannot install a factory or grant acquisition to
the Telegram importer. Support still requires compiled registration, actual
implementation/version evidence and the [review cycle](../connectors/review-policy.md).

Telegram passes with unknown coverage and no deletion signal. The synthetic
adapter passes with own-messages-only coverage and explicit tombstones. These
different expectations are intentional and do not promote Telegram to complete
history or to live synchronization. Existing dedicated store, metadata, Bridge
and Project tests remain separate regression coverage.
