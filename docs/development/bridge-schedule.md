# Telegram Bridge: offline refresh scheduler contract

`core/src/bridge_schedule.rs` is the pure scheduling decision for
`tgsum-i1w.5`. It does not call Telegram, inspect accounts or start an export.
The production desktop does **not** yet save a cadence or invoke this module;
automatic official-client export stays disabled while the OS drivers lack
installed-client evidence.

The default is `manual`. Future opt-in modes are `on_start`, `every_six_hours`
and `daily`. A scheduled attempt needs an active, unlocked GUI session, no other
export in flight, and the end of any recorded client delay/backoff. The host
must atomically persist `RefreshCheckpoint::try_claim` under its Project/global
export lease **before** asking a driver to act. A crash or ambiguous timeout
leaves `unresolved_attempt = true`, which refuses unattended retries across
restarts until an explicit resolution. A known terminal outcome may clear it;
`resolved_with_backoff` delays retries after known failure/cancellation. The
configured intervals are product choices, not platform-safe rate limits.

The contract is checked with a fake clock/session in
`core/tests/bridge_schedule.rs`: manual default, lock, single global lease,
restart, unknown outcome, delay/backoff and backwards clock. These tests
qualify only the scheduler decision; they do not qualify UI automation or a
Telegram account. Runtime wiring and fake-client UI tests remain in
`tgsum-i1w.5`/`tgsum-i1w.7` after a driver is viable.
