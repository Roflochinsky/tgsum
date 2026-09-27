# Saved privacy controls and local comparison

`tgsum-af2.7`, 2026-09-27. The Project source screen configures optional
categories, private terms and attachment choices before preparing context.
No messenger acquisition, account authorization or inference destination changes.
The existing [privacy pipeline](bundles.md) and dated [PII](pii.md),
[infrastructure](infrastructure.md), [terms](custom-terms.md) and
[attachment](attachments.md) reviews apply.

## Persistent policy

`ProjectChange::Privacy(PrivacyProfile)` atomically saves the preset, validated
`privacy_options` and custom terms under the existing revision/CAS contract.
Project schema **7** adds saved options. Versions 1–6 without options open with
the old default: mandatory secrets, other categories disabled. Opening does not
rewrite old revisions. Nondefault options cannot masquerade as a legacy schema.

| Preset | Categories |
| --- | --- |
| `secrets` | Mandatory high-confidence secret rules |
| `people` | Secrets + participants, emails, phones, usernames |
| `work` | People + all seven infrastructure categories |
| `custom` | Explicit category selection |

Host/domain dictionaries, custom terms, literal exceptions and the choice to
redact medium-confidence candidates remain independently configurable. Named
presets validate their category sets. A category checkbox switches to `custom`.
High-confidence secret scanning is always enabled; medium candidates stay
pending unless their redaction is selected. Pending findings block Export/Run.

`keep_values` contains exact, case-sensitive **whole detected values** to leave
in optional PII/infrastructure output. It cannot exempt a secret or an explicit
custom term. A longer exempted participant alias shields its contained shorter
aliases. It does not remove older private mappings or aliases. Limits: 256
unique entries, 32 KiB total, 4096 bytes/entry; no empty entries, surrounding
whitespace or control characters. Errors/Debug omit the values.

`prepare_saved_bundle` uses the policy in the expected Project revision.
Tauri `prepare_project_bundle` uses it when `options` is omitted; explicit
options still provide a caller override. The desktop always uses the saved
policy. Publishing new pseudonyms can advance the Project revision, so it
reloads the Project after preparation.

Unsaved policy edits immediately invalidate the visible bundle and prepared
analysis ticket. Saving a source preserves an unsaved policy draft. Failed
validation/conflict retains it; explicit reload or reopening the Project loads
committed values. A source draft must be saved before preparation. The backend
independently enforces the reviewed revision for comparison/export/analysis.

## Selected files

`attachment_catalog(project, revision, source, offset)` returns up to 50 items
from the **saved selected message scope**, with at most 8 KiB of metadata per
item. It does not open the attachment root or files and never downloads URLs.
Eligibility means a supported local reference, not that the file exists.

The UI lets the user choose the export folder and individual text references.
Unsupported references are disabled. Saved choices bind message ID, attachment
position and exact metadata; changed metadata requires a new selection.
Source save/reconnect keeps those choices. The root picker resolves the user's
explicit folder choice; subsequent reads use the existing no-follow reader.
Archive extraction, attachment execution and credential discovery are absent.

Review lists the exact generated content files and sizes, included counts,
missing selected files and choices outside the current message scope.
`attachment_references - included_attachments` is the count of references whose
content was not included. A missing choice never silently becomes an included file.

## Local before/after comparison

`review_items(project, bundle, revision, offset)` pages through evidence (50
items). `preview_evidence(..., reference)` explicitly compares one item:

- Message `before`: text from its immutable snapshot.
- File `before`: securely reopened from the recorded selected root, only when
  its actual byte count and SHA-256 match the version captured at preparation.
  Changed or unavailable files show that state and no original text.
- `after`: quoted text from the retained, digest-checked sanitized artifact.
  It stays available when the external original changes/disappears.

Each pane is bounded to 24 KiB at a UTF-8 boundary. Skipping large unrelated
messages does not create an unbounded preview line. Cancellation and revision
checks apply before returning data. Private bundle schema **7** adds exceptions
and evidence document pointers. Old retained bundles remain resolvable; old
message bundles without a pointer require new preparation for comparison.

This operation is distinct from `resolve_evidence`, which still never reopens
the original attachment. No additional raw file copy is retained for preview.
Raw comparison data/labels remain local IPC/UI data and never join public
manifest files, analysis arguments or UI preferences. The WebView uses
`textContent` and clears comparison text on policy/scope changes.

The existing minimum sanitized preview and findings still appear before any
Run. Review reports actual replacements by category across the whole bundle,
including repeated occurrences, plus pending secrets. These are not counts of
unique people/values or proof of complete anonymization.

## Verification

Public store tests: `privacy_profile.rs`, `attachment_catalog.rs`,
`privacy_preview.rs`; desktop mock IPC: `commands.rs`. They cover persistence,
legacy opening, preset validation, secret/custom-term precedence over literal
exceptions, bounded catalog/comparison, stale revision, changed/missing originals
and historical evidence resolution.

Start an isolated synthetic Tauri host using the environment in
[onboarding.md](onboarding.md), then run:

```sh
PYTHONDONTWRITEBYTECODE=1 uv run --with websocket-client \
  python scripts/ui-privacy-smoke.py 9263
```

The script refuses an ordinary backend or nonempty Project store. It creates
temporary synthetic archives/files, uses public IPC to seed the selected root,
and exercises the actual forms, pending/applied findings, file selection,
comparison, invalidation of a synthetic Run, persistence and validation errors.
Native folder dialogs are a separate manual fixture check. Linux results do
not qualify Windows/macOS UI or real accounts. Real-account tests remain under
the user's control.

The native GTK folder picker was also exercised in the isolated test host:
its own AT-SPI Open action selected `/tmp`, the field received the canonical
path and preparation was disabled until source selection was saved. No files
in that directory were imported by this picker check.
