# Selected local text attachments

Backend contract for `tgsum-af2.6`, 2026-09-27. Primary-source review:
[Telegram metadata and filesystem boundary](../research/text-attachments-2026-09-27.md).
The importer still records references without opening files. Bundle preparation
can now include explicitly selected local text files. No messenger client,
account, URL fetch, archive extractor or script interpreter is involved.
Desktop selection/preset controls belong to `tgsum-af2.7`.

## Selection and source scope

`SourceSelection.attachments` defaults to `None`. An explicit selection contains:

```json
{
  "root": "/absolute/export/root",
  "files": [{
    "message_id": "123",
    "position": 0,
    "expected": {"...": "exact canonical Attachment metadata"}
  }]
}
```

`expected` is the complete `snapshot::Attachment`, including its path,
availability and optional metadata. Select it from an imported snapshot; do not
invent a reference from a filename. Choices and root stay in private Project
configuration and local editing IPC. They are never public bundle metadata.
Use the ordinary `ProjectChange::Selection` revision/CAS operation (Tauri
`update_project`); changing the selection invalidates existing Review for export.

Only attachments of messages included by source/topic/date/service/delta scope
are read. Unselected references do not open any source file. A chosen position
whose metadata changed fails preparation and needs reselection. Choices outside
the emitted messages are not read: `attachment_choices_outside_scope` reports
their count. This includes vanished positions and unchanged messages excluded
by `only_changes`; metadata-only snapshot diff cannot discover file-byte edits.
A matching reference can survive reimport when its canonical metadata is equal.

Telegram paths are relative to the export root beside `result.json`, not the
chat subdirectory. `file_name` is descriptive metadata, never the opening path.
The flat importer now reads official `photo_file_size` (legacy `photo_size`
remains an alias), `file_size` and the existing document fields. Availability
placeholders do not become local filenames. Photos, nested/rich attachments,
media conversion and archive extraction are outside this text packager.

Project schema **6** adds attachment selection. Versions 1–5 without choices
migrate in memory without rewriting old bytes; only a later write creates v6.
A legacy-version manifest cannot activate attachment choices.

## Filesystem boundary

Implementation uses `cap-std` and `cap-fs-ext` **4.0.3**, locked in Cargo.lock.

- Save the canonical target of the explicitly selected root (for example macOS
  `/private/tmp/...`, not its `/tmp` alias). Opening walks the saved absolute
  path from `/` or a local Windows drive root with directory handles and
  `open_dir_nofollow`. UNC and device roots are unsupported.
- Validate reference syntax and text extension **before** opening the root.
  References use `/`, at most 4096 bytes and 64 components. Empty/`.`/`..`
  components, absolute/drive/UNC forms, backslashes, colons/ADS, control
  characters, Windows reserved characters/device names and trailing dot/space
  are rejected. Names are not URL-decoded or Unicode-normalized.
- Walk each relative directory component without following symlinks, then open
  the leaf read-only with no-follow and Unix nonblocking flags. Check the opened
  handle is a regular file before reading. Windows also checks the reparse bit
  on every opened directory/file. Links inside the root are rejected too.
- Read in bounded chunks from that handle with cancellation checks. Enforce the
  actual byte count, regardless of advertised archive size. Changed size/mtime
  during reading fails preparation. SHA-256 describes the bytes actually read.
- Output is newly written, never a hardlink or source-name copy. No source tree
  path is passed to an agent; `AGENTS.md` becomes a quoted generated document.

The root path authorizes its **current** directory after restart, not a persisted
inode identity. Directory handles prevent symlink redirection but do not freeze
the tree or defeat privileged mount changes. Source hardlinks do not prove
exclusive ownership. Concurrent same-size in-place writes can evade metadata
checks: the captured bytes are not a guaranteed atomic filesystem snapshot.
Filesystems may themselves perform remote I/O; this component issues no HTTP
requests and cannot certify arbitrary filesystem drivers/device-open effects.

## Text, privacy and budgets

Allowed extensions (ASCII case-insensitive):

`txt md log json jsonl yaml yml toml ini csv rs py js jsx ts tsx go java kt kts
c h cc cpp hpp cs rb php swift scala sql sh bash zsh ps1 css scss html xml conf cfg`.

Full UTF-8 decoding is required. Preserve a UTF-8 BOM; reject invalid UTF-8,
NUL, escape and other control characters except tab/CR/LF. MIME/extension alone
do not prove textual or safe content. No charset guessing, execution or parsing
as agent configuration occurs. Every line is quoted after the same secrets →
PII → infrastructure → custom-terms pipeline used for messages. Medium findings
use `field: attachment_text` with the attachment's evidence reference and block
export/analysis until a redacted Review is prepared.

| Resource | Bound |
| --- | --- |
| Saved choices | 100 per Project, unique message/position within each source |
| One input file | 8 MiB actual bytes |
| Inputs in one bundle | 32 MiB; repeated references count separately |
| Sanitized attachment artifacts | 64 MiB total |
| Privacy field | Existing scanner limit: 16 MiB |
| Project / public JSON / index line | Existing 1 MiB / 4 MiB / 4 MiB limits |

Each attachment stays whole in a separate document; message part-size estimates
do not split it. Adapter input limits still apply to the entire prepared bundle
and can be smaller than these packaging limits. No silent truncation occurs.

## Manifest, evidence and repeatability

Public files are `attachment-00001.md`, etc. alongside `context-00001.md` and
`manifest.json`. The manifest lists reviewed sizes/digests, included count and
opaque records for selected files. Missing files/root produce visible `missing`
records; unsafe paths/types, access failures and budget excess fail preparation.
Unselected references remain marked `file not included` in message documents.
Private paths, original names and raw-content hashes stay outside public output.

An attachment has its own `a_…@r_…` evidence. Its ID binds the native message key
and position; its revision binds the canonical message revision, attachment
metadata and actual input size/hash through the Project's keyed evidence codec.
Different copied bytes change the revision even when the JSON snapshot did not.
The private JSONL index binds this evidence to the parent snapshot and reviewed
sanitized artifact. Private bundle schema **6** recognizes those bindings;
schemas 1–5 remain readable without inventing included files.

`resolve_evidence` returns the immutable parent message and, for file evidence,
`ResolvedAttachment` containing the checked sanitized document and private input
digest/size. It verifies the stored artifact, never reopens the original file.
Historical resolution therefore survives source deletion. Export and both agent
request builders use only manifest-listed generated files. Recipe owner/deadline
quotes may cite attachment evidence, and the managed result lifecycle validates
those references through the same private index.

Preparation failure/cancellation discards its staging bundle and does not move
the Project revision or analysis baseline. Existing private-store durability,
same-user tampering and retention limits are documented in [bundles](bundles.md).

## Verification scope

Synthetic Linux fixtures cover selection/restart, metadata changes, missing and
out-of-scope files, traversal/URL/reserved names, leaf/parent/root symlinks, FIFO
with a subprocess deadline, directory and binary rejection, count/byte budgets,
privacy Review, independent copies, evidence history and managed recipe results.
Mock Tauri IPC covers all privacy stages on a selected file. Both Codex/Claude
request builders receive only reviewed documents without launching any CLI.
See `core/tests/attachments.rs`, `runner/tests/attachments.rs` and the combined
privacy fixture in `src-tauri/tests/commands.rs`.

These Linux checks do not qualify Windows/macOS filesystem behavior or real
client exports. Expanded race/collision/rollback tests are the separate
`tgsum-af2.9` scope; real-account validation remains under user control.

Verification on 2026-09-27: `bash scripts/check.sh quick` and full
`bash scripts/check.sh` passed (270 tests, 27 existing environment-specific
ignored tests). Rust 1.88.0 core tests passed (183, none ignored); the same
toolchain compiled core with `cargo check -p tgsum-core --locked --target`
for `x86_64-pc-windows-gnu` and `aarch64-apple-darwin`. Those last two checks
establish compilation only. The FIFO subprocess is counted once in the totals.
