# Privacy qualification

`tgsum-af2.8`, 2026-09-27. All data is synthetic. Tests use public scanner,
Project/snapshot, Review/Export and desktop IPC boundaries. No accounts,
credential validation, client automation or external inference are involved.
Detector grammars and limits remain in [sanitization](sanitization.md),
[PII](pii.md), [infrastructure](infrastructure.md), [terms](custom-terms.md) and
[privacy controls](privacy-controls.md).

## Fixed regression thresholds

| Corpus | Required result |
| --- | --- |
| `fixtures/secrets-v2.json` | 82 cases: 62 detected positives, 20 unchanged negatives, 0 misses/false positives; 50 high, 12 medium; all 13 rule families |
| `fixtures/infrastructure-v1.json` | 54 cases: 31 positive, 23 unchanged negative; exact strings/counts; all 7 categories |
| `fixtures/privacy-pipeline-v1.json` | 12 cases through saved Work policy: 7 positive, 3 unchanged negative, 2 explicitly documented limits; each as both message and selected file |

These numbers describe the checked-in examples. They are not a measured
precision/recall estimate for arbitrary conversations. The pipeline's two
known-limit examples deliberately stay unchanged: an unobserved personal name
and base64-encoded data. They are not counted as true negatives. Keep-values
also deliberately retain explicitly chosen values.

The secrets corpus now includes GitHub, JWT and generic-key cases previously
covered only by the scanner tests. No detector rule or rules version changed.
Expected counts/category coverage prevent silently weakening the corpus by
dropping a family or negative group. IDs must be unique. Changes to these
thresholds need an explained change to the expected supported behavior.

## Full pipeline checks

`privacy_quality.rs` checks independently written expected text and counts for
both original message text and its selected UTF-8 attachment. The composed
cases cover all four PII and seven infrastructure categories, secret/term
precedence, literal exceptions, generated labels versus identical source text,
Unicode and normalization aliases, public URL boundaries and ordinary
engineering text. Category counts include repeats, both bodies and a sender
where present.

Each case reopens the Project store and prepares again: the reviewed Project
revision, public manifest, file digests and preview must match. Export must
contain exactly `manifest.json` and its named content files, with no original
attachment names/paths, raw synthetic secret canaries, evidence key or private
selection metadata. Original text is checked only through the explicit local
comparison operation. The private mapping remains available through the private
store interface; it is not exported.

Existing public lifecycle suites additionally cover ambiguous equal names,
distinct native identities, reordered imports, malformed/unsupported contacts,
scope exclusions, Unicode boundaries, category independence, mapping aliases,
custom-term overlaps, budgets, pending findings, cancellation and stale Review.
They remain part of the focused command below rather than being copied into a
second implementation-shaped suite.

The desktop combined-policy IPC test now carries selected sanitized files
through synthetic Prepare/Run/read/list. Public review/result/history responses
must not contain source names, participant values, terms, unused private host
configuration or original attachment names. The synthetic result validates the
normal recipe/evidence contract without launching an agent.

## Diagnostic regression found and fixed

The new public regression first failed because derived `Debug` on
`AttachmentChoice` / `AttachmentSelection` printed raw metadata and the selected
root. Those values propagated into Project and stored-analysis diagnostics.
Their `Debug` implementations now expose only position/count. The test checks
direct choices, Project, RunRequest, RunTicket and StoredAnalysis formatting.
Private serialization still retains the required source bindings for recovery.

This is a guarantee about these attachment-policy diagnostics. Raw snapshots,
local original previews and private Project serialization intentionally contain
source data; arbitrary application logging is not automatically sanitized.

## Reproduce

```sh
cargo test -p tgsum-core --locked \
  --test privacy_quality --test secrets_corpus --test sanitize \
  --test infrastructure --test infrastructure_bundle \
  --test pii_bundle --test pii_contacts --test custom_terms \
  --test privacy_profile -- --nocapture
cargo test -p tgsum --all-features --locked --test commands \
  combined_privacy_policy_prepare_returns_the_revision_required_for_export
bash scripts/check.sh quick
bash scripts/check.sh
```

Any golden/count/coverage/leak regression fails the normal Cargo gate. A passing
subset does not certify the others. Native Windows/macOS, real messenger
fixtures and actual AI account behavior remain separate qualification tasks.
