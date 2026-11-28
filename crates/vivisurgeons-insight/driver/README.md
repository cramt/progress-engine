# insight driver

Improves Vivisurgeon's Insight one batch at a time. A batch is one Claude Code
thread, run by a T3 Code scheduled task, following the project skill
`.claude/skills/insight-batch/`. The thread is the writer; it also dispatches
the blind reviewers. Everything a script can decide (measuring, gating,
pushing or parking, triage, metrics, stopping) is decided here, in
subcommands the thread calls in order:

```sh
nix develop --command node crates/vivisurgeons-insight/driver/src/main.ts <subcommand> --state-dir <dir>
```

Node runs the TypeScript directly (type stripping), so there is no build step
and no runtime dependency.

## How a batch runs

A T3 Code webhook task, whose project root is the runner's own clone
`/pool/dev/progress-engine-insight` (on main), opens a Claude Code
thread (Opus, full access) with the prompt:

> Run one insight batch with the insight-batch skill, state dir /pool/dev/progress-engine-insight-state

nixconf's `services.insight-driver` clones it if it is missing and
fires the webhook every 30 minutes while the Claude pool has surplus. It
skips while `state.json` has `stopped` set or T3 Code lists a live thread in
the clone's project. Whether a batch is alive is T3's thread status, never
a timeout: a claim outlives a dead thread, and a slow batch outlives any fixed
limit. The contract it relies on:

- `start` sets `inFlight` (with `startedAt`) before anything slow.
- `triage`, the batch's last command, clears it. So does every exit 3 after
  `start` set it.
- A stop only a human can clear sets `stopped: {reason, at}` and writes the
  reason to `escalations.md`. Every `start` then exits 3 until the human runs
  `resume`.

## Subcommands

| Subcommand | Does | Exit |
|---|---|---|
| `start` | Preflight: on `--branch` (default `main`; any other branch also takes `origin/main` in and pushes that), clean, equal to origin, fast-forwarded first when it is only behind (someone else pushed); a commit origin lacks stops it. A conflict stops. Then a release build and the baseline (`insight coverage`, `roundtrip`, `lint --counts`, `snapshot`) in `batches/<n>/`, and prints the writer's brief (`brief.md` rendered with the decisions, method notes, queue, the milestone's failing list and the handoff; also saved as `batches/<n>/brief.md`). | 0 brief printed; 3 stop |
| `start --dry-run` | Baseline and brief into `<state>/dry-run/`. No git changes; `state.json` untouched. | 0 |
| `gate` | The gates against the batch's baseline; prints the report and where the batch stands against its commit cap and hours. Saves the run, so `finish` needn't repeat it at the same HEAD. | 0 pass; 1 fail |
| `finish` | Reads `batches/<n>/report.json` (summary, resolved findings, escalations). No commits: empty. Gates passed at HEAD: pushes, rebasing and re-gating if origin moved, then builds the review inputs and prints their paths. Otherwise parks the commits on `insight/failed-<n>` and resets. Says when the method review is due. | 0; 2 bad report.json; 3 git stop |
| `triage <findings-*.json ...>` | Validates each reviewer's findings, updates the queue and backlog, appends the `metrics.jsonl` line, bumps `state.json` and clears `inFlight`. | 0; 2 bad file |
| `resume` | Clears `stopped` and the consecutive-failure count. The human's step. | 0 |

A subcommand that finds the batch at the wrong step exits 2; anything thrown
exits 70.

Stops (exit 3, `stopped` set): two failed, unpushed, empty or abandoned
batches in a row; a milestone at 100% waiting to be pinned; every milestone
pinned; a preflight git refusal; git refusing to park a batch (only off main or with a
dirty tree: parking resets main to origin/main wherever origin has moved,
and drops nothing that isn't on the parked branch). Also
exit 3 without touching state: `stopped` already set, or a batch in flight
while another top-level thread in the project is live in `t3 thread list`
(the caller's own, `T3CODE_THREAD_ID`, and sub-agents aside).

A batch found in flight with no such thread lost its thread. The next
`start` records it: at the write step its commits are parked on
`insight/abandoned-<n>`, main goes to origin/main, and it counts as a failure; past `finish` it is
recorded without findings. If its last gate passed at the HEAD it died on, on
a clean tree, the new batch adopts the branch: after its baseline is measured,
`start` cherry-picks the commits origin doesn't already hold, and the brief
says so, so the new batch's gates and reviewers judge them as its own. A
conflict leaves main at origin and escalates the branch.

## The gates

Git: on the branch, clean, none of the batch's commits already on origin,
`insight: ` subjects, no AI attribution, `decisions.md` byte for byte as the
batch started, and no milestone's golden file pinning a different number of
cards (`insight pin` is the human's step; blessing trees leaves the count
alone). Then `cargo fmt --all --check`, `cargo clippy --all-targets -- -D
warnings`, `cargo test -p vivisurgeons-insight` (which holds every tree in
`golden/` and `snapshot/`), and the ratchets against the baseline: round-trip
failures, abilities parsed, lint per kind the baseline has (a kind landed on main
mid-batch is left to `cargo test`'s own lint baseline), and no lost tree in the snapshot
diff. If the Scryfall bulk changed mid-batch, the baseline is measured again
in a worktree at the baseline commit.

A push rejected because the branch moved fetches and rebases. A clean rebase
is gated again against a fresh baseline measured at origin in a worktree,
then pushed, up to three rebases. A rebase over commits that touch only
`.claude/` (the overseer's or the other product's skill edits) skips the
re-gate: nothing built or measured reads them. A conflict, a failed re-gate, a fourth
rejection or a git error parks the commits on `insight/unpushed-<n>`, resets
to origin and escalates. After a rebase, review and metrics compare against
origin's baseline, so the human's commits are not counted as the batch's. A
`finish` that finds HEAD already rebased past the baseline (a push that
rebased and died before pushing) measures the baseline at the origin commit
it sits on, for the same reason.

Every `git fetch` and `git push` that fails on the network (a host that
won't resolve, a dropped connection: `transientNetwork` in `src/sh.ts`) is
retried after 15s, 45s, 2m and 5m before it counts as a failure. b45's
`finish` crashed on a DNS blip with its gate passed and sat unpushed.

The current milestone is the first in `milestones.json` not both at 100% and
pinned, where pinned means `golden/<name>.txt` holds as many cards as the
milestone has. A list that grows reopens its milestone. A list added
mid-batch, with no row in the batch's baseline, waits for the next batch.
`mtgo-logs` comes first: the cards that keep the most cached Magic Online
games from being read whole, so Chancellor's replay has those games to play
(`chancellor-logs coverage --top 150`), ranked by games blocked: its `# ranked` line makes `insight failing` keep the list's order instead of EDHREC's.

## Review and triage

Review inputs are every new tree, an even sample of up to 300 changed
trees, and every relexed card, in files of about 90. A relexed card has a
line that still doesn't parse whose text as the lexer leaves it changed
(`insight snapshot` writes those lines to `<snap>.unparsed`), shown beside its
Oracle text from the bulk file: a rewrite before the parse, like a short name
read as `~`, has no tree for the other two kinds to show. b47's first-name
rule turned Dragon Cultist's "red Dragon creature token" into `~` that way,
and no reviewer saw it. The skill gives each to
a sub-agent, three at a time, told to read only `src/ast.rs`, `SEMANTICS.md`
and its input, and to write `findings-<label>.json`. Triage sends regressions
and game-changing findings to the head of `queue.jsonl`, minor ones to
`backlog.jsonl`, deduplicated by card and node; findings the writer of a
pushed batch reports as resolved leave the queue first. Every 3 batches the
skill runs a method review sub-agent that rewrites `method-notes.md`.

## State (`--state-dir`)

| File | What |
|---|---|
| `state.json` | `nextBatch`, `consecutiveFailures`, `lifetimeCostUsd` (frozen, see below), `inFlight` (the batch's step, `startedAt`, baseline and last gate run) and `stopped`. |
| `driver.log` | Every subcommand's log lines, the same ones it prints to stderr. |
| `handoff.md` | The writer's notes to the next one; the thread updates it every batch. |
| `decisions.md` | The supervisor's decisions, in every brief. Only the human edits it. |
| `method-notes.md` | Written by the method review; in every brief. |
| `queue.jsonl`, `backlog.jsonl` | Findings waiting for a writer. |
| `escalations.md` | Decisions for the human: card exclusions, milestones to pin, unpushed or abandoned batches, git refusals, stops. |
| `metrics.jsonl` | One line per batch: `batch`, `outcome` (`pushed`, `failed`, `unpushed`, `empty`, `abandoned`, `refused`), `startedAt`, `endedAt`, `commits`, `overall`, `milestone`, round-trip, lint, findings, queue, `usage` (tokens, from 2026-11-24; `costUsd` before batch 27). |
| `batches/<n>/` | Baseline and after snapshots, `brief.md`, `gates-<sha>.md`, `report.json`, and `review/` with the inputs and findings. |

## What the loop driver did that this one doesn't

- **Cost in dollars.** A thread can't read its own spend, so `costUsd` is
  `null` from this driver on and `lifetimeCostUsd` stays where the loop
  driver left it. `finish` records tokens instead, read from Claude Code's
  transcripts of the clone over the batch's window (`src/usage.ts`, which
  also runs alone for any window, Chancellor's batches included). Batches 27
  to 43 have neither. There is no per-batch cost cap; the quota gate is what
  bounds spend.
- **Hooks.** Pushing, pinning and editing `decisions.md` were denied as the
  writer tried; now they are rules in the skill, and the gates catch each
  after the fact. The reviewers' read-only-three-files rule is the prompt's
  alone. Past the commit cap or the hours nothing stops the writer;
  `gate` prints both.
- **Restarts.** Each subcommand is a fresh process, so a `start` that takes
  in a main changing the driver needs no restart: later subcommands run the
  new code, and `start` tells the thread to re-read the skill.
