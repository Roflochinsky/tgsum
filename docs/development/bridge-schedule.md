# Telegram Bridge: offline refresh scheduler contract

`core/src/bridge_schedule.rs` contains the scheduling decision and durable
claim/lease contract for `tgsum-i1w.5`. It does not call Telegram, inspect
accounts or start an export. Project schema v9 stores `telegram_refresh` per
selected Telegram JSON source. Old manifests open with an empty map and a
manual default; they are rewritten only on the next Project edit. The desktop
does **not** yet invoke the scheduler or expose automatic refresh in its UI;
official-client automation stays disabled while the OS drivers lack
installed-client evidence.

The default is `manual`. Future opt-in modes are `on_start`, `every_six_hours`
and `daily`. A scheduled attempt needs an active, unlocked GUI session, no other
export in flight, and the end of any recorded client delay/backoff.
`TelegramExportLease::try_acquire` uses one OS file lock beneath the Project
root, across projects and processes. While holding it, `claim` publishes an
immutable Project revision with an unresolved checkpoint **before** a future
driver action. Any concurrent Project edit, including a client or destination
change, makes the claim fail without starting a client action. A crash or
ambiguous timeout leaves `unresolved_attempt = true`, which refuses unattended
retries across restarts until an explicit resolution. A known terminal outcome
may clear it; `resolve_with_backoff` delays retries after known
failure/cancellation. The configured intervals are product choices, not
platform-safe rate limits.
If a restart loses the in-memory attempt, the host can call
`resolve_after_client_review` only after the user checks that Telegram Desktop
is no longer exporting. It requires the displayed Project revision and exact
unresolved checkpoint, so an intervening Project edit forces a fresh review.
Recovery clears only the blocked attempt and applies a 15-minute retry backoff;
it does not claim that an archive was imported or that the export succeeded.
Any archive left by the interrupted attempt must go through the normal
assisted-import validation separately. The desktop does not expose this review
flow yet, because automatic export remains disabled.
The lock uses [`fs4`](https://docs.rs/crate/fs4/1.1.0), whose synchronous
feature supports the project's Rust 1.88 minimum.

`core/tests/bridge_schedule.rs` checks the manual default, session lock,
global lease across independent processes, restart and reviewed recovery, unknown outcome,
delay/backoff, backwards clock, schema migration and removal of assisted
settings. These tests qualify only
local scheduling; they do not qualify UI automation or a Telegram account.
Runtime wiring, driver terminal-outcome mapping and fake-client UI tests remain in
`tgsum-i1w.5`/`tgsum-i1w.7` after a driver is viable.
