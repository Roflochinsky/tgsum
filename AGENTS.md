# tgsum: agent workflow

Use Beads (`bd`) for project tasks and durable working memory. Run commands
from the repository root. Communicate with the user in concise Russian.

## Start or resume

1. Inspect `git status --short --branch` and the current diff before editing.
2. Run `bd prime` and `bd list --status in_progress`. Resume the relevant issue
   with `bd show <id>`; use `bd ready` when choosing new work. Claim an existing
   task with `bd update <id> --claim`, or create one before implementation.
3. Record the requested outcome, acceptance criteria, and constraints in that
   issue. Recover decisions and the next step from its notes. Verify stale
   claims against the current files, commands, and CI before relying on them.

## Project context

- `core/`: streaming parser, topic reconstruction, Markdown formatting/output.
- `src-tauri/`: desktop commands; `src-tauri/ui/`: static HTML/CSS/JS, no npm build.
- Preserve local processing, bounded-memory parsing, exact integer IDs, UTF-8,
  and lossless splitting of messages. Relevant behavior is in the current
  source/tests and the v0.2 revision of `docs/specs/tgsum.md`.
- `docs/plans/tgsum.md` is the retired Node plan; it is not the implementation
  plan for current work. Setup and launch commands are in `README.md`.

## Memory and continuity

- Beads is the task ledger. Keep progress in the issue's `--notes`: findings,
  decisions and reasons, checks/results with revision or run ID, next action,
  and blockers. Update after a meaningful milestone and before a handoff;
  replace stale status instead of accumulating contradictory summaries.
- Save reusable lessons with `bd remember --key <stable-key> "<fact and source>"`.
  Search with `bd memories <keyword>`; update an existing key when facts change.
  Keep specifications and research in `docs/`, linked from the issue/memory.
- Link existing GitHub Issues when relevant. Use Beads for execution tracking;
  creating public issues/comments or migrating trackers needs a user request.
- The `.beads/` database stays out of source commits. Its remote sync is
  separate from `git push`; see README's agent-memory section for a fresh clone.
  If Beads fails, report the command/error and continue independent work;
  restore the ledger before claiming that progress or memory was saved.

## Execute a task or `/goal`

- For a multi-step task, keep a short sequence of verifiable milestones in the
  Beads issue. Make scoped changes, run the relevant check, repair failures,
  then update the record. Preserve user changes and other workers' files.
- Continue routine authorized work without another permission round. Ask only
  for missing input that materially changes the result; work on independent
  parts while waiting. Follow the active task/skill's delegation rules.
- A `/goal` belongs to its thread. Preserve its full outcome through retries
  and compaction; record continuity in Beads. Follow the runtime's lifecycle
  and budget controls. Before completion, match every acceptance criterion to
  current evidence. Passing a subset of checks proves only that subset.

## Verify

| Change | Required evidence |
| --- | --- |
| Rust | `bash scripts/check.sh quick` after edits, focused behavior tests, then `bash scripts/check.sh` on the final tree |
| Cargo or check scripts/CI | Full `bash scripts/check.sh`; relevant script/workflow validation |
| Behavior fix | Regression test that fails without the fix |
| UI | Run `cargo run -p tgsum` and exercise the changed flow; Rust checks do not cover HTML/CSS/JS behavior |
| Shell scripts | ShellCheck commands in README |
| Documentation/instructions only | Verify facts, links and documented commands; `git diff --check`; for agent instructions, verify loading in Codex |

`scripts/check.sh` is the shared local/CI implementation. Format with
`cargo fmt --all`. Fix diagnostic causes; a targeted lint exception needs a
reason beside it. `bash scripts/check.sh core` cannot certify the desktop app.
After relevant gates pass, repeat or broaden checks only for new changes or
an unresolved risk. Report environmental failures and unverified scope.

## Finish and publish

Record the final evidence in Beads and close only completed issues. This
repository's maintainer workflow permits committing and pushing the task branch
and syncing Beads after validation, subject to the user's current constraints.
Use `bd dolt commit` then `bd dolt push` for the database; use a normal Git commit
and push for source files. Verify both results. Preserve other workers' stashes
and branches; merge to `main`, releases, and deployments need an explicit request.
Report the outcome, actual checks, remaining scope, branch/commit, and sync errors.

When changing this workflow or model settings, read the dated sources and
applicability limits in [the SOL 6 research](docs/research/openai-sol6-agent-workflow.md).
Model/effort selection is a client setting, not an effect of this file.

## Connectors and data use

Before adding a source, changing acquisition/auth/API/SDK or archive formats,
automating a messenger client, changing retention, or handing data to an AI tool,
follow [the connector review cycle](docs/connectors/review-policy.md). Update its
registry entry and Beads evidence before promoting support. Read
[the gateway design](docs/specs/context-gateway.md) for new pipeline work and
[CONTEXT.md](CONTEXT.md) for domain terms. Keep planned capabilities distinct
from shipped behavior; registry metadata alone does not enforce runtime policy.
