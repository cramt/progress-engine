---
name: insight-batch
description: Run one unattended batch of Vivisurgeon's Insight grammar work (write, gate, push, blind review, triage). Use when a prompt says to run an insight batch.
---

# One insight batch

You are the writer of one batch, and nobody is watching. The driver's
subcommands decide everything a script can: the baseline, the gates, pushing
or parking, triage and metrics. You write grammar, dispatch the reviewers and
keep the handoff. A batch ends when `triage` says nothing is in flight.

## Nobody answers

The first message is the only one a person sends. Ending a turn ends the
batch, and the runner starts the next one in a new thread minutes later. So
never end a turn to ask, to hold, or to wait for a go: nobody will give it.
Every choice is yours. Make the call the brief and the handoff point to,
write it in the handoff, and keep working. Questions that really are the
human's go in report.json's `escalations`, and the batch carries on.
Waiting on a background subcommand is not a pause: you are woken when it
ends, and you carry on from there. A batch ends only where the steps below
say so.

## Running the subcommands

Every subcommand is, from the repo root (your cwd, the runner's clone, on main):

```sh
nix develop --command node crates/vivisurgeons-insight/driver/src/main.ts <subcommand> --state-dir <state dir>
```

`<state dir>` is the one your prompt names. `start`, `gate` and `finish` build
and measure for longer than a Bash call may last: run them with
`run_in_background` and wait for the completion notice. stdout is the
result; stderr is the log. Exit 70 is a crash: report its stack and stop, unless the
stack is a git fetch or push that failed on the network (the driver already
retried it for ~8 minutes): then run the same subcommand once more first.

## Rules for the whole thread

These were enforced by hooks once; now they rest on you, and the gates catch a
breach after the fact:

- Commit locally only. `finish` pushes; a manual push fails the batch.
- Leave pinning to the human: `insight pin` moves a golden file's card count,
  which fails the gates.
- `decisions.md` is the human's file: read it, report against it in
  report.json, and leave its bytes as they were. Grammar questions are yours
  to settle in SEMANTICS.md (the brief says how); escalate only exclusions and
  what the tool is for.
- Never `pkill -f` or `pgrep -f ... | kill` a pattern: it matches your own
  shell's command line and kills it. Batch c28 did this twice, and the second
  time it killed a mutation loop before the loop restored `game.rs`. Find the
  PID in one call and kill it in the next. Wait for any background loop that
  restores files to finish before you start a gate.
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
   first; `gate` prints both. Every new rewrite, resolver or lexer rule gets
   a negative test beside its positive one: a phrase just outside its
   SEMANTICS row that the guard must refuse. Make it a near miss: it carries
   the rule's own keyword ("from", "can't") so it reaches the new arm, and
   the filter is what refuses it. Check by widening the filter: if the test
   still passes, it guards nothing (b49's enters-from test had no "from").
   Reviewers only see trees that moved, so a guard broader than its row is
   otherwise caught only when a later card trips it. Then sweep for
   siblings: grep the snapshot's unparsed lines for the phrase's variants
   (another subject, "or"-joined, another zone) and take or note each one.
   b49's if-can't rule left "If the player can't" unread on 4 cards. Done when the tree is clean and every commit carries its
   measured counts.
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
