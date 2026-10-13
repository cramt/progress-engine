# insight driver

A deterministic loop that improves Vivisurgeon's Insight with Claude agents
run through the Claude Agent SDK. It replaces the LLM supervisor of
`/tmp/insight/supervisor.md`: every decision that a script can make
(measuring, gating, pushing, resetting, triage, stopping) is made here in code.
The agents only write grammar, review trees and revise the method notes.

## Run it

From the repo root, inside the flake devshell (node and pnpm come from it):

```sh
nix develop --command pnpm install
nix develop --command node crates/vivisurgeons-insight/driver/src/main.ts --dry-run
nix develop --command node crates/vivisurgeons-insight/driver/src/main.ts --max-batches 3 --max-cost-usd 50
```

Node runs the TypeScript directly (type stripping), so there is no build step.
`--dry-run` takes the baseline, renders the writer prompt, runs every gate and
builds the review inputs, then stops. It calls no agent, pushes and resets
nothing, and leaves `state.json` alone. Add
`--compare-snapshot <old snapshot>` to build review inputs from a real diff.

A real run refuses to start unless `main` is checked out, clean, and equal to
`origin/main`.

## One batch

1. **Baseline.** A release build, then `insight coverage`, `roundtrip`,
   `lint --counts` and `snapshot`, recorded in `batches/<n>/baseline.json`.
   The current milestone is the first one in `milestones.json` without a
   `golden/<name>.txt`. At 100% it is escalated for a human to review and pin,
   and the loop stops.
2. **Writer.** One `query()` with `prompts/writer.md`, filled in with the
   handoff, the supervisor's `decisions.md`, the queued findings, the
   milestone's `insight failing` list and `method-notes.md`. A `PreToolUse`
   hook denies `git push`, `insight pin`, any edit, write or shell command that
   touches `decisions.md` and, once the wall clock (`--writer-hours`) or the
   commit cap (`--commit-cap`) is reached, every tool except reading and
   editing the handoff. The AbortController stops the call 20 minutes after
   that. Writers report a decision they carried out in their summary, not in
   the file.
3. **Gates.** Git hygiene (on main, clean, none of the batch's commits
   already on `origin/main`, `insight: ` subjects, no AI attribution);
   `origin/main` moving past the baseline by itself is a human's push, left
   to the push's rebase below. Then `cargo fmt --all --check`,
   `cargo clippy --all-targets -- -D warnings`,
   `cargo test -p vivisurgeons-insight`, which holds every tree in `golden/`
   and `snapshot/`, so a batch that moves trees must commit them blessed
   (`UPDATE_GOLDEN=1`, as the writer prompt says), then the ratchets against the
   baseline: round-trip failures, abilities parsed, lint per kind, and no lost
   tree in the snapshot diff. If the Scryfall bulk changed mid-batch, the
   baseline is measured again in a worktree at the baseline commit. On a pass
   the commits are pushed. On a fail, one fix-up `query()` gets the gate
   output (and `decisions.md`). If the gates still fail, the commits are parked on
   `insight/failed-<n>` and main is reset to the baseline. The reset runs only
   if the baseline is an ancestor of HEAD and `origin/main` still equals it.

   A push rejected because someone else pushed to main first (the human
   commits by hand) fetches and runs `git rebase origin/main`. A clean rebase
   is gated again: a fresh baseline is measured at `origin/main` in a
   worktree, and the round trip, coverage and lint may not regress against
   it, nor may any tree it parses be lost. Then the push is retried, up to
   three rebases. A conflicted rebase (aborted), a failed re-gate, a fourth
   rejection or a git error parks the commits on `insight/unpushed-<n>`,
   resets main to `origin/main`, writes an escalation and records the batch as
   `unpushed`, a failed batch. None of it stops the loop. After a rebase, the
   review and the metrics compare against the `origin/main` baseline, so the
   human's commits are not counted as the batch's.
4. **Review.** Every new tree and an even sample of up to 300 changed trees,
   with their Oracle text, in files of about 90. Reviewers run three at a
   time. A hook lets each one read only `src/ast.rs`, `SEMANTICS.md` and its
   own input. Each reviewer returns structured findings.
5. **Triage.** Regressions and game-changing findings go to the head of
   `queue.jsonl`, minor ones to `backlog.jsonl`, deduplicated by card and node.
   Findings the writer of a pushed batch reports as resolved leave the queue.
6. **Metrics.** One line in `metrics.jsonl`.
7. **Method review**, every `--method-every` batches: one `query()` reads the
   metrics and the latest findings and returns the new `method-notes.md`. The
   driver writes that file; the agent has no write tools.

Stops: `--max-batches` and `--max-cost-usd` for this run, two failed or empty
batches in a row (failed, unpushed or empty; rerun with `--reset-failures` after looking), a milestone
ready to pin, or a git state the driver won't reset.

## State (`--state-dir`, default `/tmp/insight/driver/`)

| File | What |
|---|---|
| `state.json` | Next batch, consecutive failures, lifetime cost, and the batch in flight with its step. A rerun resumes from it. |
| `driver.log` | One line per step, the same lines as stdout. |
| `handoff.md` | Copied from `--handoff-source` on first run. The writers keep it up to date. |
| `decisions.md` | The supervisor's decisions, part of every writer and fix-up prompt. Only the human edits it; the writer hook denies any change. |
| `method-notes.md` | Written by the method review; part of every writer prompt. |
| `queue.jsonl`, `backlog.jsonl` | Findings waiting for a writer. |
| `escalations.md` | Decisions for the human: card exclusions, milestones ready to pin, unpushed batches, git refusals. |
| `metrics.jsonl` | One line per batch (`outcome`: `pushed`, `failed`, `unpushed`, `empty`, or `pushed-manually` for one a human pushed after a crash), and one per method review. |
| `batches/<n>/` | Baseline and after snapshots, prompts, gate output, agent transcripts (`*.jsonl`), and `review/` with the inputs and `findings-*.md`. |

## Flags

`--writer-hours 3`, `--commit-cap 15`, `--fixup-minutes 45`,
`--method-every 3`, `--review-chunk 90`, `--review-changed-cap 300`,
`--review-parallel 3`, `--failing-shown 120`, `--model`, `--review-model`,
`--claude-path` (defaults to the CLI binary bundled with the SDK),
`--handoff-source`.

## Agent settings

Writers load the repo's project settings (CLAUDE.md) and run with
`bypassPermissions`, because nobody is there to answer a prompt. Reviewers and
the method review load no settings and run with `dontAsk` and read-only tools.
Every call removes the sub-agent tools (`Agent`, `Task`, `Workflow`), sets
`attribution: false`, and caps `maxBudgetUsd` at what is left of
`--max-cost-usd`. A call that hits the hard wall clock returns no result
message, so its cost is not counted; the soft limit in the hook exists so that
calls normally end before that.
