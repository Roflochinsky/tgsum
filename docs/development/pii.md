# Participants and contacts in selected bundles

Backend contract for `tgsum-af2.3`, 2026-09-27. Known senders, their observed
display aliases and supported contact shapes use private Project mappings.
Desktop preset controls are described in [privacy controls](privacy-controls.md)
(`tgsum-af2.7`). This does not change acquisition or use real accounts.

## Interface and identity

`BundleOptions.pii` defaults to an empty category set. The currently supported
independent opt-ins are:

```json
{"redact_candidates": true, "pii": {"categories": ["participants", "emails", "phones", "usernames"]}}
```

Tauri's `prepare_project_bundle` accepts the same options. Secrets remain a
separate mandatory stage. The processing order is secrets → PII →
selected infrastructure rules → saved [custom terms](custom-terms.md).
Generated PII labels are protected from the term stage. Legacy one-shot Markdown
export is unchanged.
Enabling only contacts does not discover or replace participants, including when
a Project already retains PERSON mappings. Participant sender fields use their
native identity before contact-shape rules. Prose contacts are indivisible for
participant matching even when their contact category is disabled.

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
labels follow deterministic field/batch order (identities within each allocation
batch are sorted); independently created Projects need not use identical numbers.

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

URL, email/SSH-like contact, phone, `@handle` and path-shaped containers protect their
contents from participant alias matching. This includes malformed URL candidates
and quoted absolute paths. Guards do not validate contacts or paths and do not
anonymize them: other category rules own that content. Names containing controls,
empty names and aliases containing the secrets replacement marker are not used
for prose matching. Their sender fields can still be replaced by native ID.

## Email shapes

EMAIL identity is `pii/1/email`, exact local-part bytes and the domain normalized
by the locked `url::Host` parser (case and IDNA). `Alice` and `alice` remain
distinct; no provider-specific plus/tag or dot merging runs. Unicode and ASCII
punycode forms of the same accepted domain share a label. Email identity is
Project-wide, independent of the conversation/platform where it was mentioned.

Supported shapes have a nonempty dot-atom-like local part of at most 64 UTF-8
bytes and a dotted DNS-shaped domain. Unicode local-part bytes are retained;
controls, whitespace, leading/trailing/consecutive dots and unsupported ASCII
syntax are rejected. Domain labels are 1–63 ASCII bytes after parsing, with no
edge hyphens or underscores; the whole domain is at most 253 bytes. Prose quotes,
angle brackets and terminal punctuation are retained outside the replacement.

Quoted local parts, domain literals, undotted domains and malformed candidates
are outside coverage. Broad lexical candidates prevent replacing just a valid
ASCII suffix of an unsupported address. URLs (including `mailto:`), direct
SSH/SCP/SFTP operands and paths are excluded; optional infrastructure rules own
those containers. These are conservative text shapes, not a complete RFC email
validator or proof of deliverability. No network resolution runs.

## Phone shapes

PHONE identity is `pii/1/phone`, normalized `+digits` and an optional extension.
Only explicit `+`, a first ASCII digit 1–9 and 8–15 total ASCII digits are
accepted. Spaces, tabs, NBSP/narrow NBSP, dots, hyphens and balanced non-nested
parentheses are presentation separators. Wrapping punctuation stays outside the
replacement. Obvious signed `YYYY-MM-DD`/`YYYY.MM.DD` shapes are excluded.

Supported extension markers are `ext`, `ext.`, `extension`, `x`, `доб`, `доб.`
and `;ext=` (case insensitive). Word markers require a boundary; `x123` is also
supported. An extension has 1–10 ASCII digits, retains leading zeros in its
identity, and is hidden with the base number. Equal base/extension pairs reuse a
label; a different extension or no extension is a different endpoint. An explicit
malformed/duplicate supported marker rejects the candidate instead of masking
only its base.

The lower bound, extension bound and date exclusion are TGSUM heuristics, not
E.164 requirements. No region is guessed: national numbers, `00`, Unicode digits,
vanity numbers and unsupported formats remain outside coverage. Oversized digit
runs are not truncated to a valid prefix. Candidates do not cross newlines or
adjoining words; URL/`tel:`, email, handle and path containers are protected.
Phone recognition does not establish ownership, assignment or reachability.

## Standalone usernames

USER identity is `pii/1/username`, platform, account-local namespace and handle.
Conversation is excluded. Telegram handles use ASCII lowercase; other platform
namespaces retain exact case without claiming platform-specific validity.
Supported free-text shapes are `@` plus 1–32 ASCII letters, digits or underscores.
This lexical range accommodates short handles; it does not verify registration
or implement every platform's username grammar. Unicode, dotted, hyphenated,
oversized or other malformed shapes are not partially replaced.

Email/SSH, URL, phone and path containers shield their contents; `@handle` in
prose quotes remains detectable. Source titles, message bodies and sender/service
fields receive explicit source scope. A handle in unscoped Project metadata is
replaced with `[REDACTED_USERNAME]` and counted unresolved; no platform identity
or private USER entry is invented for it.

USER is independent of PERSON: matching spelling never links a handle to a
participant. Handles can change owners; their repeated spelling is evidence of
the same observed handle, not the same human. No typed entity target is recovered
from flattened text and no network identity lookup runs.

## Mapping, Review and export

Participants, contacts, infrastructure and custom terms share one private
`MappingDraft`. Discovery and
replacement allocate in memory; preparation stages one immutable map and commits
one Project revision only after bundle files, preview and cancellation checks.
Repeated preparation without new entries/aliases leaves the revision unchanged.
Generic-only replacements need no mapping and do not create one. Use the returned
Review revision for export/analysis. Baselines never advance during preparation.

Cancellation, resource failure and a lost CAS preserve the current map pointer;
the bundle is discarded. A late failure can leave an unreferenced immutable map.
An I/O error during Project publication requires reloading state, as documented
in [mapping persistence](pseudonyms.md). No automatic orphan cleanup is added.

Private bundle schema 5 retains the `pii_policy`/mapping binding introduced in
schema 4 and additionally binds any Project dictionary; schemas 1–4 remain
readable. Public `manifest.pii` records rules version `pii/1`, enabled categories,
total/per-category replacements, ambiguous and unresolved counts (including
unscoped usernames). It contains no
names or native identities. Export verifies the policy/summary/version binding.

Medium-secret finding positions are projected through both PII and
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
Contact scanners also bound each candidate to 16 KiB and attempts to 100,000 per
category/field. Unsupported bounded shapes are skipped; exceeded budgets fail
preparation without publishing the mapping. These rules do not guarantee removal
of every identifier, arbitrary PII, entity in an attachment or disguised contact.

`core/tests/pii_bundle.rs` exercises public ProjectStore/SnapshotStore interfaces:
collisions, unresolved senders, observed renames, repeated preparation/restart,
Unicode/case/platform/account distinctions, overlapping aliases, protected
containers, selected dates, shared mapping publication, Review offsets, private
export exclusion, budgets and late cancellation/CAS. Tauri command tests exercise
all category options and returned revision. `core/tests/pii_contacts.rs` covers
email case/IDNA, complete contact shapes and punctuation, phone extensions and
false positives, username scope/case growth, restart and disabled categories,
unscoped generic redaction, shared publication, secret Review/export exclusion
and contact budgets. Fixtures
are synthetic; no preset UI or real-account qualification is implied.

Primary facts and implementation decisions are in the
[dated research](../research/participant-pii-2026-09-27.md).
