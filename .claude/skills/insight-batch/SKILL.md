---
name: insight-batch
description: Run one unattended batch of Vivisurgeon's Insight grammar work (write, gate, push, blind review, triage). Use when a prompt says to run an insight batch.
---

# One insight batch

You are the writer of one batch, and nobody is watching. The driver's
subcommands decide everything a script can: the baseline, the gates, pushing
or parking, triage and metrics. You write grammar, dispatch the reviewers and
keep the handoff. A batch ends when `triage` says nothing is in flight.

## Running the subcommands

Every subcommand is, from the repo root (your cwd, the runner's clone, on main):

```sh
nix develop --command node crates/vivisurgeons-insight/driver/src/main.ts <subcommand> --state-dir <state dir>
```

`<state dir>` is the one your prompt names. `start`, `gate` and `finish` build
and measure for longer than a Bash call may last: run them with
`run_in_background` and wait for the completion notice. stdout is the
result; stderr is the log. Exit 70 is a crash: report its stack and stop.

## Rules for the whole thread

These were enforced by hooks once; now they rest on you, and the gates catch a
breach after the fact:

- Commit locally only. `finish` pushes; a manual push fails the batch.
- Leave pinning to the human: `insight pin` moves a golden file's card count,
  which fails the gates.
- `decisions.md` is the human's file: read it, report against it in
  report.json, and leave its bytes as they were.
- No AI attribution in any commit (Co-Authored-By, "Generated with").
- You are the only writer. Sub-agents are for blind review and the method
  review alone: a nested writer measured ~3% of trees wrong in game-changing
  ways.

## Steps

1. **Start.** Run `start`. Exit 3 means stop: report its line and end the
   thread; it has already written any escalation. Exit 0 prints the brief
   (also at `batches/<n>/brief.md`). Done when you have read the brief whole,
   and re-read this file if it says main changed the skill.
2. **Write.** Work the brief: its order of work, its rules, its gates. Stop at
   about 15 commits or 3 hours from the brief's start time, whichever comes
   first; `gate` prints both. Done when the tree is clean and every commit
   carries its measured counts.
3. **Gate.** Run `gate`. Exit 0: go to step 5. Exit 1: the report lists each
   failure (also in `batches/<n>/gates-<sha>.md`).
4. **Fix up, once.** One attempt of about 45 minutes, then `gate` again and
   go on to step 5 whatever it says: `finish` parks a batch that still fails.
   Fix forward with new commits, or amend or drop this batch's commits; never
   touch a commit before the baseline. A rule that lost trees or raised a lint
   count is narrowed or reverted, not patched cleverly. Note what broke and
   why in `handoff.md` for the next writer.
5. **Report.** Write `batches/<n>/report.json` in the state dir:
   `{"summary", "resolved": [{"card", "node", "verdict": "fixed" | "not-a-defect"}], "escalations": [{"kind": "exclusion" | "decision", "cards", "question"}]}`.
   `summary` has the measured counts and every supervisor decision you carried
   out with its commit hash. `resolved` copies `card` and `node` exactly as
   queued, only for findings still fixed at HEAD. Done when it parses.
6. **Finish.** Run `finish`, with no commits after the last `gate`. It pushes
   a passing batch (rebasing and re-gating if origin moved), or parks a
   failing one on `insight/failed-<n>` and resets. Exit 2 names what to fix
   in report.json; exit 3 is a git stop: report it and end the thread.
7. **Review**, when `finish` lists review inputs. Dispatch one sub-agent per
   input, three at a time, each with the prompt in
   [`reviewer.md`](reviewer.md) filled in. Done when every input has its
   `findings-<label>.json`, or a reviewer failed twice (triage records it as
   unreviewed).
8. **Method review**, when `finish` says it is due: one sub-agent with
   [`method-review.md`](method-review.md) filled in. Done when
   `method-notes.md` and the rationale file are written.
9. **Handoff.** Update `handoff.md` in the state dir: gotchas you hit, open
   findings you did not close, what you half-understand. Keep it tight; the
   next writer reads all of it.
10. **Triage.** Run `triage <every findings-*.json>` (none for a batch that
    did not push). Exit 2 names a malformed file: have its reviewer redo it,
    or drop it. Done when it prints that nothing is in flight.
11. **Summary.** End with the outcome, the counts, the findings and anything
    escalated.
