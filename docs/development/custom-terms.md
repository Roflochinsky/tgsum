# Private sensitive-term dictionaries

Backend contract for `tgsum-af2.4`, 2026-09-27. A Project can retain an explicit
dictionary of company names, project names, domains, repositories or other text.
The bundle Review shows the replaced text. The desktop dictionary
editor and presets are described in [privacy controls](privacy-controls.md).

## Configure and retain

Use the existing `ProjectStore.update` / Tauri `update_project` interface:

```json
{
  "projectId": "project-EXAMPLE",
  "expectedRevision": 3,
  "change": {
    "kind": "custom_terms",
    "value": {
      "entries": [
        {"value": "Project Apollo", "boundary": "word"},
        {"value": "customer.example", "boundary": "substring"}
      ]
    }
  }
}
```

The updated Project contains `custom_terms`. This is private configuration;
local editing IPC intentionally returns it to the caller. `CustomTerm` and
`CustomTerms` Debug formatting omit the values. Policy deserialization and
validation return static diagnostics instead of quoting invalid fields/modes
or term values. They do not sanitize arbitrary source data or caller logging.

Project schema 5 adds the dictionary, empty by default. Schemas 1–4 open with
empty terms without rewriting old bytes; a later write publishes the current
Project schema (now 7 with [saved privacy options](privacy-controls.md)). A legacy-version
manifest cannot activate a nonempty dictionary.
Updates use the existing revision/CAS contract and stale any existing Review.

An empty `entries` list disables the dictionary in future preparations.
Historical Project revisions, mappings and bundles retain the previous terms;
clearing a dictionary does not erase that private history or prior exports.
Storage permissions and lack of encryption at rest follow [Projects](projects.md).
Retention/deletion remains its own approved scope.

## Matching and identity

A nonempty saved dictionary is always applied during bundle preparation. There
is no second per-bundle option that could silently omit it. Legacy one-shot
Markdown export has no Project dictionary and is unchanged.

- Matching uses exact UTF-8 bytes and case. There is no Unicode normalization,
  case folding, transliteration, stemming, regex input or automatic alias learning.
- `word` is the default. Neither neighboring character may be a Unicode `\w`
  character, including combining marks. This is a lexical boundary, not language
  segmentation or a full domain/path parser. `ACME` leaves `ACMEish` unchanged;
  a configured domain can match inside a URL when those boundaries hold.
- `substring` matches the literal anywhere in source text. It can also replace
  part of a longer name, URL or path. Users explicitly choose that broader rule.
- Overlapping candidates are checked for boundaries/protected output before
  selecting the leftmost, longest valid match. An invalid longer term cannot hide
  a valid shorter one. Duplicate exact values are rejected, even with different
  boundary modes. Reordering entries does not change the selected spans.

TERM identity is `terms/1:` plus the hex SHA-256 of the exact term bytes, scoped
to the private Project mapping epoch. The digest keeps identity length bounded;
it is not an anonymization or password-protection mechanism. Original values
remain in private configuration and mapped originals. Boundary mode changes
where the term matches, not its identity. Different spellings remain distinct.

Allocation uses the shared mapping draft. Only selected matches allocate labels;
unmatched terms and longer candidates discarded by overlap checks do not.
Existing `TERM_*` labels remain stable across restart, dictionary growth, order
changes and boundary changes. New labels use the mapping's deterministic sorted
batch allocation; first appearances in different batches may allocate differently.

## Pipeline, Review and export

The order is secrets → optional PII → optional infrastructure → custom terms.
All data text fields already handled by bundle preparation participate, including
Project/source titles, sender/service metadata and selected message text. Evidence
IDs, fixed schema keys and formatting are generated metadata, not dictionary input.
Attachments are not included by this feature; their safe processing is `af2.6`.

Generated secret markers and PII/infrastructure labels are protected by their
actual replacement intervals, projected through subsequent stages. A matching
literal such as `PERSON_0001` originally present in source text is still eligible.
The detector cannot change a generated pseudonym into a different TERM identity.
Terms inside content already replaced by an earlier stage need no second mask.

Medium-secret findings remain pending even when a term replacement hides their
text. Review offsets are projected through the term stage, so excerpts use the
final transformed text. Export/analysis still require resolving all pending
findings and the exact reviewed Project revision.

Participants, contacts, infrastructure and terms publish one shared mapping
version at the end of successful preparation. Repeating preparation with no new
entries/aliases leaves the revision unchanged. Cancellation, budget errors and
concurrent edits preserve the current mapping pointer; the existing immutable
orphan-file/I/O publication limits apply as documented in [pseudonyms](pseudonyms.md).

Private bundle schemas 5–7 bind the dictionary through its exact immutable Project
revision, alongside the mapping and other policies. It does not copy the term
list into the bundle manifest. Public `custom_terms`, present only when enabled,
contains `rules_version: "terms/1"` and `replacements`. It never includes configured
terms, matching modes, identities, dictionary hashes or an alias lookup table.
Schemas 1–4 remain readable without inventing applied terms. Historical mapping
resolution uses that historical policy, not the current dictionary.

## Limits and verification

| Resource | Limit |
| --- | --- |
| Terms | 1,024 distinct values |
| One value | 4,096 UTF-8 bytes, nonempty, no controls or edge whitespace |
| Total value bytes | 64 KiB |
| Retained contiguous Aho-Corasick NFA | 64 MiB |
| Field / output | 16 MiB |
| Overlapping candidate attempts per field | 100,000 |

Project's 1 MiB serialized manifest limit and mapping limits also apply. Excess
input fails explicitly before successful publication, with value-free errors.
The locked Aho-Corasick API and Unicode matching choices reuse the checks recorded
in [PII research](../research/participant-pii-2026-09-27.md); no new dependency,
messenger acquisition, account access or network identity service is added.

`core/tests/custom_terms.rs` exercises public ProjectStore/SnapshotStore and bundle
interfaces: persistence and diagnostics, legacy migration, overlapping boundaries,
Unicode and substring behavior, generated-label provenance, revision stability,
dictionary clearing/history, concurrent edits, secret Review/export exclusion and
resource limits. Tauri mock IPC configures the dictionary and runs all privacy
stages together. These synthetic fixtures do not qualify real messenger accounts
or the later preset editor UI.
