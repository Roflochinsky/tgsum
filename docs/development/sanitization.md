# Minimum local secret sanitizer

Core preparation for `tgsum-hzm.5`, rules version `minimum-secrets/1`.
[Dated primary-source research](../research/minimum-secrets-scanner-2026-09-26.md)
records the formats and separates provider facts from TGSUM heuristics.

The Project [bundle and local Review](bundles.md) apply this scanner to every
emitted text field before export. Existing one-shot Markdown export remains
unchanged and does not claim sanitized output.

## API and output

`sanitize(text, ReviewPolicy)` returns transformed text and a report. It performs
no network requests, credential validation, filesystem access or agent calls.
High-confidence contexts are always redacted. `KeepForReview` retains uncertain
candidates and reports `needs_review`; `RedactCandidates` hides those candidates
too. Callers must resolve pending findings before publishing a bundle.

Each replacement is `[REDACTED_SECRET]`. This is not entity pseudonymization:
different secrets deliberately share a placeholder. Stable Project pseudonyms
and their private mapping belong to the expanded Privacy epic.

Reports contain version, category, confidence, action and input/output **UTF-8
byte ranges**, not raw values or secret hashes. JavaScript must not apply these
ranges as UTF-16 string indices. The transformed-text wrapper deliberately does
not implement Debug or Serialize; diagnostic logging should use its report.
The text can still contain pending findings and secrets outside current rules.

## Implemented rules

| Context | Default | Boundary |
| --- | --- | --- |
| PKCS #8, encrypted PKCS #8, OpenSSH, RSA, DSA, EC private-key markers | Redact the whole block | An unmatched opening marker hides the remaining field and reports `incomplete_private_key` |
| Explicit Authorization / Proxy-Authorization with Bearer or Basic | Redact credential value | Case-insensitive field/scheme; no fixed token length, validation or decoding requirement; next line is preserved |
| GitHub `ghs_` | Redact the whole candidate | At least 36 suffix characters; dots, underscores and hyphens supported; no maximum token-length assumption within the field budget |
| Other documented GitHub prefixes | Redact the whole candidate | At least 20 suffix characters is a TGSUM heuristic, not a provider length/validity guarantee |
| Sensitive assignments | Redact value | Password/passwd/pwd, secret, token, API/access/secret/private key, client secret, session ID; optional identifier prefixes separated by `_`/`-` |
| Generic key assignments | Review | `key` and other `_key`/`-key` names can describe ordinary data; public-key references are not automatically called secrets |
| URL userinfo with a nonempty password | Redact userinfo | `scheme://userinfo@host`; authority boundaries are preserved, including encoded userinfo and IPv6 hosts; no URL is opened |
| Standalone JWT/JWE candidate | Review | Three/five parts with a bounded decodable JSON header containing `alg`, plus `enc` for JWE; no signature/issuer/expiry claims |

Sensitive assignments support same-line quoted values, escaped quotes and
unterminated quotes through the line's end. The field name `token` remains high
in this minimum preset; the research recommendation to review ambiguous names
is a possible later refinement. This choice can hide ordinary parser-token
examples. Explicit placeholders, null/boolean values and `${...}` references
are left alone. Literal passwords starting with `$` are still hidden.

Overlapping findings produce one replacement; a high-confidence finding takes
precedence over a nested medium candidate. Bytes outside selected replacement
ranges remain unchanged. Scanning already transformed text is idempotent for
these replacements. The scanner processes full fields before truncating a
preview or splitting Markdown so that output chunk boundaries cannot split a
credential before inspection.

## Resource and coverage limits

A field above 16 MiB or more than 100,000 raw detector findings causes an explicit
error, with no partially sanitized result. JWT header decoding is capped at
16 KiB of encoded header. Repeated incomplete key markers do not repeatedly
search the same remainder for an ending marker.

Unsupported standalone forms include many provider keys, Telegram bot tokens,
cookies, recovery phrases, unlabelled passwords, PII and infrastructure names.
Obfuscated/encoded text, split messages, images and attachment bytes are outside
this module. Public-key and certificate markers are not private-key findings.
No-findings is not a claim that the text contains no secrets.

## Evidence

`core/tests/sanitize.rs` uses synthetic text only: high-confidence contexts,
long dotted GitHub installation tokens, complete/incomplete keys, JWS/JWE and
unsecured JWT review, escaped/truncated assignments, overlapping rules,
Unicode offsets, idempotence, URL boundaries, short auth values/CRLF, negative
examples, and byte/finding-budget rejection. Reports are checked for absence of
the synthetic secret values. No tests validate tokens against a service.

Bundle tests additionally verify selected-only output, redaction of metadata
and message text, refusal to export pending findings, and review excerpts for
findings beyond the bounded preview. Local Review identifies its recipient as
Export only; no agent integration is implied by these checks.
