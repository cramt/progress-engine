---
name: runner-overseer
description: Oversee progress-engine's unattended batch runner (Insight batches, later Chancellor) on a 30-minute loop - check each batch's work, merge what is good, and tune the runner until it behaves. Use when a prompt says to check in on the runner or oversee the batches.
---

# Overseeing the batch runner

The runner turns spare Claude quota into grind work on the rules engine: Insight's
grammar now, Chancellor's replay milestone next. You are its overseer. Alex
handed you the whole job: judge the work, merge it, change the runner, deploy.
The goal is a runner Alex can forget about that only gets better at the grind.

## Where things are

- **The runner** is nixconf's `modules/services/insight-driver.nix` (luna): a
  timer runs a quota gate every 30 minutes, which fires a T3 Code webhook task
  when the pool has surplus and no batch thread is live. Deploy a change with
  `just deploy luna` from a nixconf checkout. Alex's own nixconf checkout may
  be dirty or behind, so work in a `git worktree` off `origin/main`, push to
  main, then remove the worktree.
- **The batch** is the `insight-batch` skill (`.claude/skills/insight-batch/`) in
  the worktree `/pool/dev/progress-engine-insight` (branch `insight`), driven by
  `crates/vivisurgeons-insight/driver/` (its README is the contract).
- **State:** `/pool/dev/progress-engine-insight-state/`: `state.json`
  (`inFlight`, `stopped`), `driver.log`, `metrics.jsonl` (one line per batch),
  `batches/<n>/` (brief, report, gates, findings), `escalations.md`,
  `handoff.md`.
- **Threads:** `t3 thread list --project /pool/dev/progress-engine-insight --json`
  and `t3 thread show <id> --messages N`. A thread showing `completed` with
  `pendingBackgroundTasks` is still working.
- **Gate log:** `journalctl --user -u insight-driver-gate -n 20`.
- **Your notes:** `/pool/dev/progress-engine-insight-state/overseer.md`. Append
  one dated entry per check-in that saw something: what you saw, what you
  changed and why. Read the latest entries first each tick; they are your
  memory across ticks and context compactions.

## Each check-in

1. **Read** your latest notes, the gate log, `state.json`, the batch threads and
   any new `metrics.jsonl` line. If nothing moved since the last entry and
   nothing is wrong, end the tick in one line.
2. **A batch in flight:** look at its commits so far (`git log` on the
   worktree from `inFlight.baseline.head`). If one is clearly wrong, tell the
   thread with `t3 thread send <id> "<note>"`: the evidence (Oracle text from
   Scryfall, the before and after tree), not a command. Keep the writer free;
   no hooks, no caps.
3. **A batch finished:** check its work independently before trusting its
   reviewers. Dispatch one sub-agent to verify the moved trees against Oracle
   text (the brief that found Explosive Revelation's regression: every card
   whose tree moved, verdict better / same / worse / still wrong, root cause
   for anything worse). Then decide:
   - **Merge** `origin/insight` into main (merge or fast-forward, never squash)
     when nothing got worse, or when what got worse is queued for the next
     batch and the gain clearly outweighs it. Run `cargo test -p
     vivisurgeons-insight` on the merge result first.
   - **Hold** it and say why in your notes when it regressed something
     without a fix in sight.
4. **Stopped for a human** (`state.json` `stopped`): read `escalations.md`.
   Settle it yourself if it is a judgement the overseer can make (a milestone
   to pin, a failure you can diagnose), then `resume`. DM Alex (`dm-me`) only
   for a decision that is really theirs.
5. **Tune the runner** when the data says so: the brief, the skill, the gates,
   the gate's timing, the scope. Change one thing at a time, say in your notes
   what you expect it to move, and check on a later tick whether it did.

## Rules

- Merge, deploy and change the runner without asking; that is the job.
- DM Alex only when you need them. Routine check-ins stay in the thread.
- Never touch the worktree while a batch is in flight; read it only. Make your
  own changes on main in `/pool/dev/progress-engine` (commit by path; other
  agents may be working there) or in a separate worktree.
- `decisions.md` in the state dir is Alex's: read it, never change it.
- A runner change that moves a tracked number or behaviour updates the
  driver README, this file or the module's header comment in the same commit.
