# Development feedback

Run commands from the repository root. The shared check implementation is
`scripts/check.sh`; CI uses its `lint` and `test` modes.

1. After a Rust change, run `bash scripts/check.sh quick`. Fix compiler errors
   before continuing. Run focused tests for the changed behavior, for example
   `cargo test --locked -p tgsum-core --test parse`.
2. Before handing off Rust, Cargo, or check-infrastructure changes, run
   `bash scripts/check.sh` on the final tree. Completion requires exit code 0.
   For a behavior fix, add a regression test that fails without the fix.
3. If a check fails, fix the cause and rerun it. Format with `cargo fmt --all`.
   Preserve the gates: any targeted lint exception needs a concrete reason
   next to it; broad suppression or skipped tests are not fixes.
4. Report the commands actually run and their results. If dependencies or the
   environment prevent a check, report the exact blocker and the unverified
   scope. `bash scripts/check.sh core` is useful without GUI libraries, but it
   does not verify the desktop app or replace the full gate.

For tool installation, check modes, and common Rust errors, read
[README.md — Проверки для разработки](README.md#проверки-для-разработки).
For UI changes, also run the app and exercise the changed flow; Rust checks
do not validate HTML/CSS/JS behavior.
For shell script changes, also run the ShellCheck commands listed in README.md.

`docs/plans/tgsum.md` describes the retired Node implementation. Use the current
Rust source and the v0.2 revision in `docs/specs/tgsum.md` for current behavior.
