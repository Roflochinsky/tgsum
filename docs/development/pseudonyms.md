# Private Project pseudonyms

Implemented storage contract for `tgsum-af2.5`, 2026-09-27. The core can allocate,
retain, resolve and reset labels. Infrastructure rules now use this mapping
during opted-in bundle preparation; see [infrastructure](infrastructure.md).
Known [participant and contact replacement](pii.md) also uses the shared draft.
[Saved custom terms](custom-terms.md) use the same draft; Privacy preset/reset
controls remain work in the Privacy epic. Creating a mapping alone does not replace strings: replacement
requires an enabled detector. Legacy one-shot export is unchanged.

## Interface and identity

`ProjectStore` owns persistence:

- `assign_pseudonyms(project_id, expected_revision, inputs)` returns labels in
  input order and the updated Project. Each input has `category`, `identity` and
  `original`. The result's revision is required for subsequent Review.
- `load_pseudonyms(project_id, reference)` loads exactly the referenced version.
  The returned local lookup has `lookup(category, identity)` and
  `originals(label)`. It cannot be serialized or accidentally printed with Debug.
- `reset_pseudonyms(project_id, expected_revision)` publishes an empty new epoch.
- `bundle_pseudonyms(project_id, bundle_id)` resolves the mapping pinned by a
  historical bundle; legacy unbound bundles return `None`.

Mapping schema 1 uses exact UTF-8 identity bytes with no case folding, Unicode
normalization, hostname parsing or inferred identity merging. Detectors must
define their canonicalization and namespace explicitly. A participant identity
must include its platform/account scope and stable sender ID where available;
a changed display name is a new alias of that identity. Equal display names
do not merge distinct identities. Same bytes in different categories remain
distinct. Changing a detector's identity rules needs a versioned migration.

Within a Project epoch, labels are sequential per category: `PERSON_0001`,
`HOST_0001`, `IP_0001`, etc. New identities in a batch are sorted before
allocation; duplicates return the same label. New batches append labels and
aliases without renumbering existing entries. Retrying an identical import
leaves the Project revision unchanged. Allocation order across separate batches
can affect new labels; reordering an already observed corpus cannot rename them.

Projects have independent dictionaries and random epochs. The same readable
label can mean different things in different Projects/epochs. A bare label is
therefore insufficient for historical resolution; use the bundle/version binding.

## Private format and publication

```text
projects/project-.../
  revisions/00000000000000000004.json   # schema 5, optional MappingRef + terms
  pseudonyms/
    map-<128-bit random ID>.json       # immutable full mapping generation
  bundles/bundle-.../
    private.json                      # schema 5, exact MappingRef + Project/policy binding
    context/manifest.json             # optional opaque pseudonym_mapping_id
```

The private reference contains `id`, `epoch`, `generation` and `sha256` of the
mapping bytes. Each file records its schema, Project ID, ID, epoch, generation
and entries. SHA-256 detects corruption relative to the reference; it does not
authenticate against a process that can modify both. The public manifest exports
only the random mapping ID. It contains no map digest, identities or aliases.
That ID binds a version; it does not claim a detector has run. Local Project
configuration and historical bundle metadata are private and do carry the full
reference.

A mutation writes a bounded temporary file inside the private directory, syncs
it and publishes it without replacing an existing name. Only then does it CAS
the Project revision with the new reference. Concurrent writers cannot overwrite
a winner. A loser returns `WouldBlock` and may leave an unreferenced map file;
the authoritative pointer is in the Project, never inferred from directory order.
Reload before retrying. An I/O error after revision publication can occur after
the revision became visible: callers must reread state, not assume rollback.

First allocation creates generation 0. Appending entries/aliases increments it.
Reset creates an empty generation 0 with a new epoch and mapping ID, then advances
the Project revision. Previous files, bundles and results remain intact. Reset
is not deletion or erasure of previously exported data. Orphan cleanup and
retention are `tgsum-6gf.4`; no garbage collection is implemented here.

Bundles verify their reference against the exact historical Project revision
and verify the referenced file. A missing/corrupt map blocks new preparation,
export and mapped historical resolution; it does not silently recreate the map
or substitute the current one. An unrelated legacy unbound bundle still resolves.
Reset leaves `evidence-key.bin` unchanged. Old Review and uncommitted analysis
become stale, while saved results retain their bundle/version and evidence.

Project schemas 1–3 load without a mapping; schemas 1–4 default to no custom
terms and migrate in memory to schema 5. The next write creates a separate
revision. Original bytes remain unchanged. Private bundle schemas 1–4 remain
readable without inventing mapping/policies/dictionary application. Unknown
versions are rejected. Older apps must reject unsupported Project schemas
instead of dropping private mapping or dictionary configuration on write.

## Limits and access

| Budget | Limit |
| --- | --- |
| Serialized mapping file | 16 MiB |
| Batch raw identity + original bytes | 16 MiB |
| Entries / inputs per batch | 100,000 |
| Total distinct aliases | 100,000 |
| Aliases per identity | 256 |
| Identity / original value | 4 KiB / 16 KiB, nonempty |

JSON escaping counts towards the serialized limit. Violations return a
value-free error before switching the current reference. Files are regular,
bounded and digest-checked; owner, IDs, epoch, generation, category labels and
entry uniqueness are validated. Symlink substitutions detected at inspection
are rejected. New Unix directories/files use `0700`/`0600`; other OSes rely on
the application-data directory's access controls.

Bundle preparation uses a private mapping draft across fields. It checks the
aggregate serialized size, alias and entry limits on each allocation without
serializing/writing a new file per field. A failed allocation poisons the draft;
partial state cannot be staged. Only one immutable map is staged when the entire
scan succeeds, and the Project CAS follows bundle and preview completion.

Mapping data is plaintext private data. Pseudonyms do not guarantee anonymity
or encrypt the Project. Memory is bounded by these budgets but holds the full
current dictionary, lookup indices and serialization buffer. Same-user hostile
filesystem races and power-loss durability on every OS/filesystem are not
claimed. No messenger/agent credentials are read and no network is used.

## Evidence

`core/tests/pseudonyms.rs` exercises restart, reordering/scope extension, aliases,
independent Projects/categories, concurrent writers/retry, reset/history, size
limits, corruption, format/owner validation and Unix permissions/symlinks through
the public ProjectStore interface. Project migration tests cover schemas 1–4;
bundle/analysis tests cover private export exclusion, legacy bundles, preserved
results/evidence and stale exports/commits. Fixtures are synthetic.

The design and dependency guarantees are documented in
[dated primary-source research](../research/project-pseudonym-mapping-2026-09-27.md).
