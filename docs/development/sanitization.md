# Local secret sanitizer

Expanded scanner for `tgsum-af2.1`, rules version **`secrets/2`**. The
[minimum research](../research/minimum-secrets-scanner-2026-09-26.md) and
[expanded primary-source research](../research/expanded-secrets-scanner-2026-09-27.md)
separate provider facts from TGSUM heuristics. Existing immutable bundles retain
their recorded sanitizer version; preparing a new bundle applies current rules.
Historical artifacts are not silently rewritten or rescanned.

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
| PKCS #8, encrypted PKCS #8, OpenSSH, RSA, DSA, EC and PGP private-key markers | Redact the whole block | An unmatched opening marker hides the remaining field and reports `incomplete_private_key` |
| Explicit Authorization / Proxy-Authorization with Bearer or Basic | Redact credential value | Case-insensitive field/scheme; no fixed token length, validation or decoding requirement; next line is preserved |
| GitHub `ghs_` | Redact the whole candidate | At least 36 suffix characters; dots, underscores and hyphens supported; no maximum token-length assumption within the field budget |
| Other documented GitHub prefixes | Redact the whole candidate | At least 20 suffix characters is a TGSUM heuristic, not a provider length/validity guarantee |
| Sensitive assignments | Redact value | Password/passwd/pwd, secret, token, API/access/secret/private key, client secret, session ID; optional identifier prefixes separated by `_`/`-` |
| Generic key assignments | Review | `key` and other `_key`/`-key` names can describe ordinary data; public-key references are not automatically called secrets |
| URL userinfo with a nonempty password | Redact userinfo | `scheme://userinfo@host`; authority boundaries are preserved, including encoded userinfo and IPv6 hosts; no URL is opened |
| Standalone JWT/JWE candidate | Review | Three/five parts with a bounded decodable JSON header containing `alg`, plus `enc` for JWE; no signature/issuer/expiry claims |
| Documented GitLab, Slack and Anthropic prefixes | Redact the whole candidate | Full prefix list in the research; ASCII letters/digits/underscore/hyphen suffix of at least 20 characters is a local heuristic. Rotated Slack `xoxe.xoxb-` / `xoxe.xoxp-` includes the outer prefix |
| Generic `sk-` (including `sk-proj-`), Google-like `AIza`, AWS `AKIA`/`ASIA` ID | Review | `sk-`/`AIza` suffix at least 20; AWS suffix at least 16 uppercase letters/digits. An ID or recognizable shape does not prove it is a secret; explicit credential fields still redact |
| Telegram Bot API `/bot…` and `/file/bot…` on exact HTTPS host | Redact token | Numeric ID plus colon plus nonempty ASCII letters/digits/underscore/hyphen suffix; no minimum length in this explicit context |
| Standalone Telegram-like token | Review | At least five ID digits and 20 suffix characters; heuristic, not a complete token grammar |
| Request Cookie / response Set-Cookie | Redact known sensitive values; review opaque candidates | Known session/auth/CSRF names are local policy; Set-Cookie inspects only its first pair. Unknown values require the entropy heuristic; attributes remain outside this detector |
| Entropy near an exact key/token/secret/password label | Review | Same-line label followed by spaces and optional `is`; value at least 20 ASCII token characters, Shannon entropy ≥3.5 bits/character. No standalone entropy scanning |

Sensitive assignments support multiline quoted values, backslash escapes and
doubled quotes in connection strings. An unfinished quoted credential hides the
rest of the field. Camel-case `accessToken`, `refreshToken`, `sessionToken` and
`idToken` are included, as are existing separator-based forms. Named headers such
as `x-goog-api-key`, `x-api-key`, `PRIVATE-TOKEN` use the assignment rule and do
not depend on a provider prefix. `GEMINI_API_KEY` covers unknown key shapes too.

The field name `token` remains high by the original preset, so ordinary parser
token examples may be hidden. Explicit placeholders and `${...}` references are
left alone. Bare `null`/`none`/boolean assignment values retain the old placeholder
policy; this can miss literal unquoted passwords with those values. Quoted values
and explicit Bearer values such as `true` are credentials and are redacted.
Literal passwords starting with `$` are still hidden.

Cookie name policy: `session`, `sid`, `sessionid`, `session_id`, `JSESSIONID`,
`PHPSESSID`, `connect.sid`, `_gitlab_session`, `auth`, `auth_token`, `access_token`,
`csrf`, `csrftoken`, `xsrf-token`, optionally prefixed by `__Host-` / `__Secure-`.
Matching is case-insensitive as a local heuristic; CSRF values are not labelled
as login credentials. Cookie headers are raw single-line text, not decoded browser
storage or recursively unescaped JSON. Values retain padding and optional quotes.
Custom short cookie values can be missed; benign high-entropy values can require review.

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

Unsupported forms include custom/new provider prefixes, short incomplete tokens,
custom short session cookies, recovery phrases, unlabelled passwords, PII and
infrastructure names. Provider token alphabets/thresholds are heuristics, not
protocol validators. Fully encoded URLs, encoded query names and signed-URL
capabilities without a recognized sensitive field are not covered here.
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

`core/tests/fixtures/secrets-v2.json` is the versioned corpus. Each case gives an
independent expected value range, category, confidence and redacted output, or
explicitly expects no finding. `core/tests/secrets_corpus.rs` checks both review
policies, full-value replacement, idempotence and report/Debug leakage. Current
result: **79 cases, TP=59, FP=0, TN=20, FN=0; 49 high / 10 medium**. These numbers
describe this synthetic corpus only, not accuracy on arbitrary user data.

```sh
cargo test -p tgsum-core --locked --test secrets_corpus --test sanitize -- --nocapture
cargo test -p tgsum-core --locked --test bundle expanded_rules
bash scripts/check.sh
```

Bundle tests additionally verify selected-only output, redaction of metadata
and message text, refusal to export pending findings, and review excerpts for
findings beyond the bounded preview. An expanded-rule regression verifies that
high provider tokens are hidden, medium candidates block export until explicit
redaction, and export leaves the analysis baseline unchanged. The scanner does
not choose a recipient; [Desktop Review](desktop-analysis.md) handles that choice.
