# Selected context and Export only

Implemented local `tgsum-ax6` flow, 2026-09-26. Projects can prepare a selected,
sanitized bundle, inspect it locally, and save its public files to a chosen
folder. This flow does not start an agent, access a messenger, or mark messages
as successfully analyzed. Legacy one-shot Markdown export keeps its old format.

## Contract

`ProjectStore` exposes three operations:

- `prepare_bundle(project_id, expected_revision, options, cancelled)` freezes
  the selected snapshots, filters them with the successful-analysis baseline,
  scans all emitted fields, and returns a local `BundleReview`.
- `export_bundle(project_id, bundle_id, expected_revision, destination,
  cancelled)` verifies the reviewed draft and copies its public allowlist.
- `resolve_evidence(project_id, bundle_id, reference)` resolves a reference to
  its exact native message and revision through the private index.

Tauri exposes the first two as `prepare_project_bundle` and
`export_project_bundle`. Arguments use camelCase; results use snake_case.
`BundleOptions.redact_candidates` selects whether medium-confidence findings
are retained for local review or hidden alongside high-confidence findings.

Preparation rejects missing snapshots, scope mismatches, an empty selection,
cancellation and concurrent Project edits. Export requires the same reviewed
Project revision and zero pending findings. Changing a source, date, topic,
privacy option or Project configuration requires a new Review. UI source edits
disable preparation until saved. Source-file changes alone require an explicit
refresh; a bundle always represents its frozen snapshot, not the current file.

## Private and public data

```text
projects/project-.../
  evidence-key.bin
  bundles/bundle-.../
    private.json
    evidence.jsonl
    context/
      manifest.json
      context-00001.md
      ...
```

Only `context`'s manifest and its named Markdown files are exported. The private
index, key, configuration, native identifiers, raw paths, exporter provenance,
credentials references and unselected message contents are excluded by schema.
Text fields such as project/source titles, names, timestamps, service metadata
and message bodies pass through the minimum sanitizer before emission.

The public manifest records sanitized titles, opaque source IDs, coverage level,
gap counts, selected date ranges/topic counts, scope/diff counts, sanitizer
version, privacy counts and file sizes/SHA-256 digests. It explicitly identifies
`export_only` as the destination. Coverage never becomes complete merely because
all selected records were written.

Attachment bytes, names and paths are not copied. Messages carry opaque
attachment references marked as not included; the manifest distinguishes their
count from `included_attachments: 0`. Safe attachment packaging is `tgsum-af2.6`.
Replies link only to targets emitted in this bundle; other targets are marked
outside-context/unresolved. Threads retain a Project-scoped opaque reference.

## Stable evidence

A Project has one random 32-byte private key. HMAC-SHA-256 over a versioned,
domain-separated JSON tuple produces opaque references. Message identity uses
the complete platform/account/conversation/message key. Revision references
also include the canonical native revision hash. Identical text with different
native IDs remains distinct; editing a message preserves its evidence ID and
changes its revision. Another Project receives independent identifiers.

The private JSONL index maps each reference to an immutable snapshot and native
key/revision. Resolution checks the index digest and recomputes the reference
from that snapshot. Old bundles therefore resolve old observations after a
refresh. A missing/corrupt key fails explicitly; restore it with the Project.
No key or native revision hash is included in exported files.

Implementation references, checked 2026-09-26:
[RustCrypto HMAC 0.12.1](https://docs.rs/hmac/0.12.1/hmac/) and
[getrandom 0.3.4 fill](https://docs.rs/getrandom/0.3.4/getrandom/fn.fill.html).
These primitives provide identifiers; they do not encrypt the private store.

## Review and privacy

All selected fields are scanned before formatting, chunking or preview limits.
High-confidence values are hidden automatically. Medium findings block export
until the user selects their redaction and prepares a new Review.

The WebView receives the first 24 KiB of Markdown, cut at a UTF-8 boundary, plus
up to 20 medium-finding excerpts and an omitted count. Excerpts come from already
sanitized text, are bounded to 251 Unicode characters including ellipses, and
can represent findings beyond the Markdown preview. Counts include the whole
selection. Finding excerpts are local Review data; they are not exported in the
manifest. UI inserts them with `textContent`.

The screen displays source/message/attachment counts, known coverage, privacy
counts and the local-folder recipient. Selected topic counts, changes-only mode,
date basis and included unknown dates remain visible. It states the scanner's limited coverage.
See [sanitization](sanitization.md): absence of findings does not prove anonymity
or absence of credentials. PII/infrastructure pseudonyms and broader provider
rules remain the Privacy epic.

## Filesystem and resource behavior

- Private JSON manifests are bounded to 4 MiB; index lines to 4 MiB. Individual
  text fields use the scanner's 16 MiB limit. Oversized data fails explicitly.
- Processing holds the current and optional baseline conversation in memory,
  plus selected references. It is not a constant-memory message-store reader.
- Markdown splits at message boundaries with the existing estimated **soft**
  token budget. An oversized single message remains whole; no hard tokenizer
  limit is promised. Text is scanned before splitting; CR/CRLF lines are quoted.
- Each exported folder has a fresh generated name. Files must be regular files,
  match the reviewed sizes/hashes and be named in the validated manifest.
  Extra cached files cannot extend the export. Symlink substitutions detected at
  inspection are rejected; the destination must be outside private Project storage.
- The output manifest is written last. Error/cancellation removes the staging
  folder. The directory exists during copying: whole-directory atomic visibility
  and directory durability across power loss are not claimed.
- Unix private directories/files use `0700`/`0600`; other platforms rely on the
  application-data directory's access controls. This does not protect against
  hostile concurrent writes by another process with the same user's privileges.
- Drafts remain private on disk, including unresolved medium candidates.
  Retention/cleanup belongs to `tgsum-6gf.4`; no automatic purge is implemented.

Markdown quotation indicates source provenance. It cannot prevent prompt
injection by itself. Agent isolation and output validation belong to the runner.

## Verification

`core/tests/bundle.rs` covers selected chats/topics/dates, duplicate text with
distinct identity, edits and historical resolution, private data exclusion,
privacy/stale-review refusal, cancellation cleanup, corrupt/symlink files,
missing keys, bounded preview with late findings, Unicode and CR-only headers.
`src-tauri/tests/commands.rs` exercises prepare/redact/export/conflict through
actual backend IPC with synthetic data. The Linux desktop flow uses an isolated
application profile and synthetic archive, including the native folder picker.
For the test app, `GTK_CSD=1` enables dialog decorations and `NO_AT_BRIDGE=0`
exposes GTK's native Open action to AT-SPI; accessibility actions are restricted
to the verified test process. The test does not alter desktop configuration.
Windows/macOS and actual agents remain separate qualification tasks.
