# Participant pseudonyms in selected bundles

Participant milestone of `tgsum-af2.3`, 2026-09-27. The backend now connects
known senders and their observed display aliases to private Project mappings.
The task remains in progress: email, phone and standalone messenger username
rules are not implemented by this module yet. Desktop preset controls remain
`tgsum-af2.7`. This does not change acquisition or use real accounts.

## Interface and identity

`BundleOptions.pii` defaults to an empty category set. The currently supported
opt-in is:

```json
{"redact_candidates": true, "pii": {"categories": ["participants"]}}
```

Tauri's `prepare_project_bundle` accepts the same options. Secrets remain a
separate mandatory stage. The processing order is secrets → participants →
selected infrastructure rules. Legacy one-shot Markdown export is unchanged.

A PERSON mapping identity is the versioned tuple `pii/1/person`, platform,
account-local namespace and native sender ID. The conversation is not part of
that identity: the same typed Telegram sender ID within one account namespace
retains its label across conversations. Another platform/account stays separate.
IDs and name bytes are not case-folded or converted to floating-point numbers.
Future connectors must supply an account-scoped native sender identity before
using this contract; a conversation-local ID alone is insufficient.

`PERSON_*` identifies an observed participant, which can also be a bot or channel;
it does not authenticate a physical person. A sender ID directly determines the
header label. Names never merge native identities. A renamed sender adds an
alias to the same private entry. Missing names still get the native ID's label;
the ID is retained privately as provenance and is excluded from prose aliases.
A literal display name equal to its native ID is also excluded from prose lookup.

A name without a sender ID is unresolved. Its header and matching prose use
`[REDACTED_PERSON]`, without allocating an invented PERSON identity. When both
ID and name are absent, the technical `Unknown sender` placeholder remains.

## Discovery, aliases and ambiguity

Preparation first scans the selected messages in each enabled source, applying
the same dates/topics/service/change filters as output. It observes sender fields
after secrets sanitization. It then builds the bounded alias dictionary before
writing any text, so later messages in the selection can establish collisions.
Discovery adds no unselected sender records to the mapping.

Current and historical aliases in this Project epoch participate in matching.
Ambiguity is conservatively **Project-wide**: the same spelling associated with
different labels, including another account/platform, cannot pick one person.
It becomes `[REDACTED_PERSON]` with an ambiguous count. Current unresolved aliases
also prevent guessing a known identity with the same name. Unresolved names
have no persistent identity; they are reconstructed from the selected messages.

Existing labels never renumber. Enlarging scope or learning a new alias can make
prose ambiguous while preserving each sender's header label. Reset starts a new
epoch; historical bundles still resolve their original mappings. Newly allocated
labels follow deterministic observation order; first allocation in independently
created Projects is not promised to use identical numbers.

Aliases match exact UTF-8 bytes and case, with Unicode word-character boundaries
including combining marks. No normalization, transliteration, name-component
extraction, declension or NER runs. NFC/NFD spellings, lookalikes and case variants
remain distinct unless they were separately observed as aliases of the same ID.
This deliberately limits prose coverage, including languages that need tailored
word segmentation. A textual alias match is not proof of real-world identity.

Literal matching uses the already locked `aho-corasick` 1.1.5, now a direct core
dependency. A contiguous NFA emits bounded overlapping candidates. Boundary and
container checks run before choosing leftmost, longest valid non-overlapping
spans. An invalid longer alias cannot hide a valid shorter overlapping name.

URL, email/SSH-like contact, `@handle` and path-shaped containers protect their
contents from participant alias matching. This includes malformed URL candidates
and quoted absolute paths. Guards do not validate contacts or paths and do not
anonymize them: other category rules own that content. Names containing controls,
empty names and aliases containing the secrets replacement marker are not used
for prose matching. Their sender fields can still be replaced by native ID.

## Mapping, Review and export

Participants and infrastructure share one private `MappingDraft`. Discovery and
replacement allocate in memory; preparation stages one immutable map and commits
one Project revision only after bundle files, preview and cancellation checks.
Repeated preparation without new entries/aliases leaves the revision unchanged.
Generic-only replacements need no mapping and do not create one. Use the returned
Review revision for export/analysis. Baselines never advance during preparation.

Cancellation, resource failure and a lost CAS preserve the current map pointer;
the bundle is discarded. A late failure can leave an unreferenced immutable map.
An I/O error during Project publication requires reloading state, as documented
in [mapping persistence](pseudonyms.md). No automatic orphan cleanup is added.

Private bundle schema 4 pins `pii_policy` and the exact mapping; schemas 1–3 remain
readable. Public `manifest.pii` records rules version `pii/1`, enabled categories,
total/per-category replacements, ambiguous and unresolved counts. It contains no
names or native identities. Export verifies the policy/summary/version binding.

Medium-secret finding positions are projected through both participant and
infrastructure replacements. Excerpts come from final transformed text. A PERSON
replacement does not silently clear a pending secret review requirement. The
existing bounded preview/excerpts and explicit redaction flow still apply.

## Limits and evidence

The participant dictionary permits 10,000 distinct prose aliases totaling 1 MiB.
It includes historical aliases; reducing the current source selection alone may
not reduce it. A reset starts a new mapping epoch without deleting history.
The matcher is restricted to a contiguous NFA with a 64 MiB retained-size cap.
Mapping budgets separately bound identities, entries and stored aliases.

Each field/output is limited to 16 MiB; overlapping match attempts and protected
span candidates to 100,000 each; each protected container to 16 KiB. Errors omit
input values. Oversized input fails before public preparation succeeds. These
limits bound processing; this is not a constant-memory snapshot reader. Enabled
participant discovery makes an additional pass over selected source snapshots.

`core/tests/pii_bundle.rs` exercises public ProjectStore/SnapshotStore interfaces:
collisions, unresolved senders, observed renames, repeated preparation/restart,
Unicode/case/platform/account distinctions, overlapping aliases, protected
containers, selected dates, shared mapping publication, Review offsets, private
export exclusion, budgets and late cancellation/CAS. Tauri command tests exercise
the combined participant/infrastructure options and returned revision. Fixtures
are synthetic; no preset UI or real-account qualification is implied.

Primary facts and proposed later contact rules are in the
[dated research](../research/participant-pii-2026-09-27.md).
