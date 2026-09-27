# Parser and store fault verification

The normal Rust gate includes a deterministic fault corpus in
[`core/tests/faults.rs`](../../core/tests/faults.rs). Run it directly with:

```sh
cargo test --locked -p tgsum-core --test faults
```

All inputs are generated or synthetic repository fixtures. Worker processes
execute the same test binary inside owned temporary directories; they do not
run messenger clients, AI agents or account requests.

## Verified invariants

| Exercise | Checked result |
| --- | --- |
| 24 generated message sets, six read fragment sizes | Unicode, escapes, repeated/empty text and native IDs through `u64::MAX` survive normalization, reload and diff. Boundary scalars exercise every UTF-8 width. |
| Every proper archive prefix and every read-error boundary, including EOF | No invalid snapshot is published, an existing snapshot stays byte-identical, normal errors remove staging files, and a later retry succeeds. |
| Every byte replaced by NUL, invalid UTF-8, a delimiter or a digit, or deleted | Malformed JSON is rejected; successful mutations reload consistently and have identical contents when read one byte at a time. The prior snapshot is unchanged. This is a bounded mutation corpus, not coverage-guided fuzzing. |
| Supported Project schemas 1–9 and invalid schema/identity/revision/selection values | Reading migrates in memory; corrupt heads cannot be edited into a new revision. A valid later write upgrades while preserving the original revision bytes. |
| Legacy snapshot corruption and every truncated prefix | Migration never repairs or rewrites corrupt storage silently. A valid legacy snapshot still upgrades in memory and retains exact IDs. |
| Project manifest at 1 MiB and one byte over; oversized attempted write | Exact boundary remains readable; overflow is rejected without advancing the head or leaving staging files. This limit applies to Project metadata, not large source archives. |
| Process exits after writing a staged observation, or after snapshot publication but before Project checkpoint | Restart retains the previous Project head. Unpublished staging is ignored. A published but unreferenced snapshot is not adopted automatically or overwritten by retry. |
| Four independent processes race on one snapshot ID or Project revision | Exactly one succeeds; three report conflict. All prior revision bytes remain intact and losing writers clean their temporary files. |

Workers synchronize before the race, have 30-second deadlines and are reaped
on failure. The crash cases use immediate process exit without Rust stack
unwinding. A real leftover staging file is inspected after the exit; it is
neither adopted nor silently removed. This verifies process restart behavior,
not power-loss durability, kernel crashes, every filesystem or disk-full recovery.

## Defect found by the corpus

Before `tgsum-66d`, the lightweight index could accept invalid UTF-8 in ignored
message fields: `serde_json::IgnoredAny` skips string decoding. The mutation
regression reproduced `index_ok=true` for a byte replaced by `0xff`; full message
deserialization rejected that particular field.

The streaming reader now checks UTF-8 in bounded chunks, including ignored
fields, retaining at most three incomplete scalar bytes between reads. Invalid
encoding and incomplete sequences fail full indexing/import. A valid buffered
prefix is delivered before a deferred encoding error so intentional extraction
can still stop after the selected chat. Such extraction continues to make no
claim about an unread tail; snapshot publication still requires validation to EOF.

Existing focused tests also cover duplicate/out-of-scope identities, immutable
publication, source matching, schema metadata tampering, corrupt committed heads
and nonblocking handling of special attachment files. This corpus supplements
those contracts; performance/resource budgets and installer/account qualification
have separate scopes in Beads.
