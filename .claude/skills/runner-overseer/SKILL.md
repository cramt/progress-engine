---
name: runner-overseer
description: Oversee progress-engine's unattended batch runner (Insight batches, later Chancellor) on a 30-minute loop - check each batch's work and fix the runner wherever it needed you, until it needs nobody. Use when a prompt says to check in on the runner or oversee the batches.
---

# Overseeing the batch runner

The runner turns spare Claude quota into grind work on the rules engine: Insight's
grammar now, Chancellor's replay milestone next. You are its overseer, for now.
Alex handed you the whole job (judge the work, change the runner, deploy), but
like a hand on the back of a bike: the goal is a runner that needs nobody, and
you are done when it does.

So **every intervention is a bug report against the runner.** When you catch
something (a regression, a stuck batch, a bad merge, a stop you had to settle),
fix the runner so it would have caught or avoided it without you: a gate, a
lint ratchet, the batch's reviewer brief, the skill, the driver, the module.
Then the work itself. An overseer who quietly fixes things by hand is training
the runner to need one.

**Letting go.** Propose ending the overseer to Alex when, over the last five
batches in a row: your independent check found nothing the batch's own gates and
reviewers missed, no stop needed you, and nothing needed a hand fix to state,
git or the module. Write the evidence in your notes first.

## Where things are

- **The runner** is nixconf's `modules/services/insight-driver.nix` (luna): a
  timer runs a quota gate every 30 minutes, which fires a T3 Code webhook task
  when the pool has surplus and no batch thread is live. Deploy a change with
  `just deploy luna` from a nixconf checkout. Alex's own nixconf checkout may
  be dirty or behind, so work in a `git worktree` off `origin/main`, push to
  main, then remove the worktree.
- **Products:** the gate offers `products` in turn (nixconf, default Insight
  then Chancellor), recording the last in `gate-last-product` in the Insight
  state dir. A Chancellor batch follows `.claude/skills/chancellor-batch/` with
  state in `/pool/dev/progress-engine-chancellor-state/` (`batches/<n>/baseline.txt`
  and `after.txt` replay reports, `handoff.md`). Its gate is the replay: the
  milestone's games to the end and lines followed never fall, divergences 0.
  Judging it means rerunning the replay yourself and checking each new rule
  against the CR, not trusting the summary. Alex asked to try both the
  alternation and Chancellor alone (`products = ["chancellor"]`); compare
  milestone movement per batch and keep what works.
- **The Insight batch** is the `insight-batch` skill (`.claude/skills/insight-batch/`) in
  the runner's own clone `/pool/dev/progress-engine-insight`, on main, driven by
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

Batches commit to main directly, as Alex's own work does: gates green means
pushed. There is no branch to merge; a bad batch is fixed forward by the next one,
with a note in the handoff or the queue.

## Each check-in

0. **Take the tick:** `.claude/skills/runner-overseer/lock.sh take`. If it
   says `busy`, another overseer tick is still running (a review outlasts the
   30-minute schedule): end in one line. Run `lock.sh release` as your last
   act. Build and replay in a `git worktree` of your own under `/pool/dev`,
   never by moving the thread's worktree: ticks share that checkout.
1. **Read** your latest notes, the gate log, `state.json`, the batch threads and
   any new `metrics.jsonl` line. If nothing moved since the last entry and
   nothing is wrong, end the tick in one line.
2. **A batch in flight:** look at its commits so far (`git log` on the
   worktree from `inFlight.baseline.head`). If one is clearly wrong, tell the
   thread with `t3 thread send <id> "<note>"`: the evidence (Oracle text from
   Scryfall, the before and after tree), not a command. Keep the writer free;
   no hooks, no caps.
3. **A batch finished:** check its work independently, then compare with what
   its own reviewers found. Dispatch one sub-agent to verify the moved trees
   against Oracle text (every card whose tree moved, verdict better / same /
   worse / still wrong, root cause for anything worse). Anything worse goes to
   the next batch as a finding. Anything you found that the batch's reviewers
   missed is a runner bug: improve their brief or add a gate, per the rule
   above. Record the comparison in your notes; it is the letting-go evidence.
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
- Never touch the runner's clone while a batch is in flight; read it only. Make your
  own changes on main in `/pool/dev/progress-engine` (commit by path; other
  agents may be working there) or in a separate worktree.
- `decisions.md` in the state dir is Alex's: read it, never change it.
- A runner change that moves a tracked number or behaviour updates the
  driver README, this file or the module's header comment in the same commit.
