# Infrastructure recognition and Project pseudonyms

Core and bundle integration for `tgsum-af2.2`, 2026-09-27. Recognition, independent
category selection and stable Project pseudonyms are connected to Prepare,
Review and Export through `BundleOptions.infrastructure`, including the Tauri
backend command. Categories default to off. Desktop preset controls remain
`tgsum-af2.7`; the existing screen does not yet enable these categories. Legacy
one-shot export is unchanged. This adds no messenger acquisition capability.

## Interface

`InfrastructureDetector::new(InfrastructurePolicy)` validates private policy
names once. `scan(text)` returns a sensitive, non-serializable scan. Its
`inputs()` feed `ProjectStore::assign_pseudonyms`; `apply(mapping)` uses that
exact loaded mapping and fails if any label is missing. The returned text has
value-free findings with category and UTF-8 input/output byte ranges. Neither
scans nor output text implement Debug/Serialize. See [mapping persistence](pseudonyms.md).

The caller must run the secrets scanner first. This detector neither removes
credentials nor declares text anonymous. Raw account/session data is never
needed. No DNS, network request, shell, filesystem access or platform client
is used to classify a value.

Core now directly uses pinned `url` 2.5.8 (already present in the workspace
lockfile). Its locked IDNA/ICU graph requires Rust 1.88, so the workspace's
declared minimum is corrected from 1.85 to 1.88. The development/CI toolchain
remains 1.98.1. Dependency metadata already required 1.88 for the desktop graph;
this change makes that requirement explicit for core consumers as well.

Policy categories are independently selectable: `ip`, `host`, `domain`, `url`,
`username`, `path`, `cloud_resource`. All are off by default. An empty selection
leaves text unchanged without invoking identifier rules. Private
`internal_domains` and `hostnames` extend the recognized scope; do not include
them in public agent manifests or logs.

## Preparation and Review

Each selected field passes through secrets rules first, optional
[participant replacement](pii.md), then infrastructure recognition. A shared
private in-memory mapping draft reuses committed labels and
allocates new ones across fields; every allocation checks the aggregate mapping
budgets. New identities are sorted within each field's batch. Source/message
order can affect labels only for previously unseen identities, never existing
ones. Preparing an already observed corpus is a mapping no-op.

Preparation stages at most one immutable map version. It writes and validates
the bundle metadata, computes the bounded preview, checks cancellation, and
finally publishes the mapping with the Project revision CAS. The returned
`BundleReview.project_revision` is authoritative for subsequent export or
analysis: allocating new entries or aliases increments it once. No-op and
unmatched selections keep the original revision. Source snapshots and successful
analysis baselines do not change during preparation.

Cancellation, budget failure or a lost CAS leaves the current mapping untouched
and removes the staged bundle. A late failure may leave an unreferenced immutable
map; it is never selected by directory order. As with other Project mutations,
an I/O error during revision publication can happen after that revision becomes
visible; reread the Project after an error. Retention/cleanup is separate work.

Private bundle schema 4 pins the exact mapping and private infrastructure/PII
policies; schema 3 infrastructure bundles remain readable.
The public manifest exposes only the random mapping ID, rules version, enabled
categories, replacement total and counts per category. It never serializes the
configured names, map entries or map digest. Export validates the binding and
historical reference. Old bundles resolve their original mapping even after a
refresh, scope extension or reset; stale Review cannot be exported as current.

Review excerpts use the final transformed text with relocated UTF-8 positions.
Nearby high-confidence secrets and replaced infrastructure stay hidden. A
medium-confidence secret finding still requires review even if a whole-URL
replacement also hides it. Choosing candidate redaction and preparing again
resolves those findings. Full-selection counts include findings beyond the
Markdown preview; the normal excerpt/count bounds still apply.

## Recognition and normalization, infrastructure/1

| Category | Recognized forms | Canonical identity |
| --- | --- | --- |
| IP | Strict IPv4/IPv6 literals, bracketed endpoints, supported IPv6 zone suffixes | Rust `IpAddr` representation; ports excluded, zone kept exactly; IPv4 and mapped IPv6 remain distinct |
| Host | Reserved local namespaces, configured hosts/domains, explicit host/server assignments and simple SSH targets | Parsed IDNA/ASCII hostname, lowercase, optional final root dot removed |
| Domain | Configured domain roots, `home.arpa`, explicit domain/dns_domain/search_domain assignments | Same hostname normalization; separate category from a host |
| URL | Parsed scheme URLs with an internal host | Pinned `url` parser serialization as identity only; original span is replaced as a whole |
| Username | Explicit user/username/login assignments and simple `ssh/scp/sftp user@host` | Case preserved; SSH identity includes normalized host; unscoped assignments use a separate namespace |
| Path | Recognizable absolute POSIX/home paths, quoted absolute paths, explicit path/file/dir assignments, Windows drive-rooted and UNC paths | Lexical identity only; drive letter and UNC host normalized separately; remaining case and `..` preserved |
| Cloud | Standalone AWS ARN, common Azure ARM scopes, Google full resource names | Service-specific lexical framing below; entire identifier replaced |

Identities use a versioned `infra/1` namespace and category. Canonical strings
above 4,000 bytes use a private SHA-256 identity to stay within the mapping's
identity budget; their original aliases still stay private. Labels retain the
existing mapping's Project/epoch semantics. Different spellings are merged only
where the documented normalization establishes an equivalence. Hostnames are
never equated with IPs through network resolution.

### URLs and overlap

The original public URL span is preserved byte-for-byte: no case changes,
default-port removal, dot-segment cleanup or query reordering is applied to
its output. Every scheme-URL candidate, including unsupported/malformed ones,
shields nested host/IP/path/username patterns. Consequently an IP toggle applies
to standalone addresses, while the URL toggle controls entire internal URLs.
This separation also applies when the URL category is disabled. Secrets must
still be handled independently before this step.

The URL parser can canonicalize equivalent spellings for private identity;
path/query case and query parameter order remain significant. Default internal
namespaces are `localhost`, `.localhost`, `.local`, `.home.arpa`, `.internal`.
Suffix checks honor label boundaries. `.corp`, `.lan`, company domains and
unqualified host aliases require explicit policy or a typed context.

Internal IP URL hosts are IPv4 private, loopback, link-local or unspecified
addresses; IPv6 ULA, loopback, link-local or unspecified addresses; and mapped
IPv6 forms of those IPv4 classes. Public numeric URLs stay unchanged. There is
no inference that every non-global address identifies a private service.

Full cloud IDs and path-shaped spans shield their nested generic patterns even
when their own category is off. Bare emails are protected from infrastructure
rules, except explicit login/SSH contexts. URL and cloud identity are distinct:
a public management API URL is not silently converted to a cloud resource ID.

### Paths and usernames

No `canonicalize`, existence check or user-home expansion runs. Windows paths
are recognized on every host OS by lexical grammar. Drive letters are folded,
while path-component case is preserved. UNC server identity is normalized;
share/path case is preserved. Relative and device paths (`\\?\`, `\\.\`)
are not treated as ordinary absolute paths.

Unquoted POSIX recognition covers familiar roots (`/Users`, `/home`, `/etc`,
`/var`, `/opt`, `/srv`, `/tmp`, `/mnt`, `/media`, `/run`, `/root`, `/workspace`,
`/Volumes`) and `~/`. Other absolute roots require quotes or a nearby path/file
context. Context lookup uses at most 128 preceding UTF-8-safe bytes. Arbitrary
HTTP routes, division and relative repository paths are not inferred to be local
private files. Matching does not promise to identify every legal filename or
shell escape. Quoted paths preserve spaces within the quotes.

SSH recognition currently covers the direct operand `command user@host`, not
a complete shell/options parser. `deploy` at two different known hosts is not
assumed to be one account; an unscoped `username=deploy` is also separate.
Username pseudonyms do not infer participant/person identity. Standalone IPv6
zones accept 1–64 ASCII alphanumeric or `_.-~` characters; unsupported captured
zone strings are not partially replaced. Interface names are never resolved by
the OS. URI scoped-zone syntax is not promised by this URL parser.

### Cloud identifiers

- AWS ARN parsing splits only the first five separators, retaining qualifiers
  and `/` or `:` inside the resource suffix. Empty region/account are accepted;
  a supplied account is 12 decimal digits. Partition/service syntax is checked;
  resource case stays exact. Wildcard resource patterns are not concrete IDs.
- Azure recognizes subscription GUID, optional resource-group scope, and
  provider/type/name forms including tenant/management-group forms. Structural
  scope words, subscription GUID and the initial provider namespace are folded;
  resource/group names are preserved. Unknown provider-specific case semantics
  are not guessed, so some variants can remain distinct.
- Google recognizes `//service.googleapis.com/collection/id/...`, preserving
  resource path case and structure while normalizing the service hostname.
  API URLs, bare UUIDs, project words and arbitrary 12-digit numbers are not
  interpreted as resource IDs.

These are lexical recognition rules, not service-side existence or validity
checks. Prose URL/path/ARN spans trim terminal `.,;` and unmatched closing
brackets; literal trailing punctuation in filenames/identifiers is ambiguous.

## Budgets and verification

Fields and resulting text are limited to 16 MiB. Individual URL/path/cloud
candidates and emitted identifier spans are limited to 16 KiB. Matches and
protected spans are limited to 100,000 each. Policy allows at most 256 configured
names totaling 32 KiB, with hostname validation after IDNA conversion. Errors
carry no original values. Mapping allocation has additional separate budgets.

Ordered interval maps prevent quadratic overlap scans; prose punctuation is
trimmed with precomputed bracket counts. Large input fails explicitly rather
than returning a partially scanned result. These bounds do not constitute a
constant-memory streaming message-store implementation.

`core/tests/infrastructure.rs` exercises the public detector and ProjectStore
interfaces. `fixtures/infrastructure-v1.json` contains 54 synthetic cases:
31 positive and 23 negative, each with literal expected output and finding
count. Additional tests cover category independence, scope extensions/restarts,
SSH account identity, Unicode, size/count budgets and diagnostic exclusions.
All corpus expectations pass; this is a regression corpus, not a measured
real-world false-positive or recall rate. No real account data was used.

`core/tests/infrastructure_bundle.rs` covers actual preparation/export, all seven
category toggles, unmatched/no-op preparation, one-version publication, private
configuration/digest exclusion, secrets-before-mapping, late Unicode Review
findings, aggregate alias limits, late cancellation/CAS conflict, refresh with
earlier newly seen messages, preserved evidence and historical mappings.
`src-tauri/tests/commands.rs` exercises the opt-in JSON policy and resulting
revision through backend IPC. These tests do not exercise a preset UI or a
real messenger/agent account.

Primary sources and uncertainty are recorded in the
[dated research](../research/infrastructure-pseudonyms-2026-09-27.md).

Verification uses the workspace gate (`bash scripts/check.sh`, Rust 1.98.1),
including Clippy and default doctest selection. A separate
`CARGO_TARGET_DIR=/tmp/tgsum-core-msrv-188 cargo +1.88.0 test --locked --offline -p tgsum-core`
run checks the declared minimum. It verifies core on Linux; it does not
qualify the desktop app or other OSes on that toolchain. No runner launch or
isolation code changed in this feature; their environment qualification remains
separate from the default gate.
