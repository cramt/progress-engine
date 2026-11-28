---
name: chancellor-batch
description: Run one unattended batch of Chancellor of the Spires rules-engine work - take the replay milestone's worklist card by card, gate on the replay ratchet, push to main. Use when a prompt says to run a chancellor batch.
---

# One Chancellor batch

You are the writer of one batch, and nobody is watching. Chancellor of the
Spires (`crates/chancellor-of-the-spires/`) is the rules engine. Its acceptance
test is real Magic Online games replayed line by line, and its milestone is
`milestones/replay-500.txt`: 500 games and their 804 cards. The batch moves
that milestone forward and never back. Read the crate's README ("Real games",
"Replay", "The milestone") and `docs/research/rules-engine.md` before writing.

Your cwd is the runner's clone, on main. `<state>` is the state dir your
prompt names. Run cargo through `nix develop --command`. Long commands (the
replay, the test suite) go in the background; wait for their notice.

## Nobody answers

The first message is the only one a person sends. Ending a turn ends the
batch, and the runner starts the next one in a new thread minutes later. So
never end a turn to ask, to hold, or to wait for a go: nobody will give it.
Every choice is yours. Make the call the skill and the handoff point to,
write it in the handoff, and keep working. Waiting on a background command is
not a pause: you are woken when it ends, and you carry on from there. A batch
ends only at step 6, or at a stop step 1 names.

Batch 18 broke this. It "held" while its gate ran, then held again when the
gate came back red, and ended its turn with uncommitted work in the tree.
Forty-five threads in a row then found the tree dirty and stopped.

## Steps

1. **Start.** Pick the batch number `n`: one more than the highest under
   `<state>/batches/`, and make that directory. Only batches write in this
   clone, so a dirty tree or commits ahead of `origin/main` are an earlier
   batch that died partway. Adopt that work and keep going. Save it first:
   `git diff HEAD > <state>/batches/<n>/inherited.diff`, and
   `git format-patch origin/main -o <state>/batches/<n>/inherited/` for the
   commits. Then `git fetch` and `git reset --hard origin/main`. If the last
   handoff or the diff says what the work was for, apply it again
   (`git apply`) and make it your first card, gated like your own. Otherwise
   name it in your handoff and leave it. Then `git pull --ff-only`. If that
   fails (the network, not the tree), report it and end the thread. Build with `cargo build --release
   -p chancellor-of-the-spires --bin chancellor-logs`, then write the baseline:
   `target/release/chancellor-logs replay --top 40 --json <state>/batches/<n>/baseline.json > <state>/batches/<n>/baseline.txt`.
   Read `<state>/handoff.md` (the last writer's notes) whole.
2. **Write.** Work the milestone's worklist from the top: the card blocking the
   most games first, unless the handoff says why not. For each, find the
   general rule the card needs, not a special case for it, as the crate's
   history does ("Thalia: cost changes", "Stifle: an ability on the stack as a
   target"). Then the card itself. A card whose stop is Insight's (a node the
   grammar reads wrong) is fixed in Insight, under Insight's own rules
   (`crates/vivisurgeons-insight/README.md`, SEMANTICS.md).
   - Every rule you implement cites the CR as "CR 613.4c", checked against the
     pinned text (`cr/`'s README says how); never from memory. Card text comes
     from Scryfall (`POST /cards/collection`) or `chancellor-logs card NAME`.
   - The engine refuses by name what it can't do. Never guess to get a line
     through: a refused card is honest, a wrong game is not.
     `unwrap_or(0)`, `map_or(0, …)`, `_ => 0`, `unwrap_or_default()` and
     their `false` kin are that guess in code: an unknown read as none.
     `tests/silent_zero.rs` fails when one is added without
     `// zero: <why zero is the rule's answer>` on it or the line above.
     Refuse instead, or write the reason; never raise the baseline.
   - A replay inference (which card a fetchland found, a choice the log never
     prints) goes in `replay.rs`, never in the engine.
   - Stop at about 15 commits or 3 hours, whichever comes first.
3. **Gate**, before each push:
   - `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`,
     `cargo test -p chancellor-of-the-spires`, and if you touched Insight,
     `cargo test -p vivisurgeons-insight` and `insight lint --counts` with no
     count above `crates/vivisurgeons-insight/tests/lint-baseline.txt`.
     Run each command on its own and read its exit status. A pipe through
     `sort`, `grep` or `tail` returns the last command's status: batch 18's
     gate passed with a failing test that way.
   - Rerun the replay: `target/release/chancellor-logs replay --top 40
     --json <state>/batches/<n>/after.json > <state>/batches/<n>/after.txt`.
     Then run the ratchet on its own and read its exit status:
     `target/release/chancellor-logs gate <state>/batches/<n>/baseline.json
     <state>/batches/<n>/after.json --allow <state>/batches/<n>/allow.txt`
     (a missing allow file allows nothing). Nonzero is red,
     whatever the totals say. It checks every milestone game, not the
     counts, because a count hides a game that finished traded for one
     that diverged. Each `FAIL` line names a game and what it did:
     - **diverges**: the engine disagrees with the log on every way the
       replay tried. A bug to fix before pushing, the engine's or the
       replay's inference (`--game ID` shows it line by line).
     - **a divergence masked by a refusal**: a way stopped at a refusal of
       a card no one named in a graveyard, and every way past it diverged.
       The refusal stops the game, but the divergence is just as wrong if
       that way isn't the game's. Fix it as a divergence.
     - **gives up going back**: the gate has already replayed the game
       going back 64 times as often as the replay does. It fails only if
       that still doesn't get past the line. Force the finds the game
       needs by hand. If it then replays past the line, it is the search:
       add `gave-up <game> <the finds forced, the line it got past>` to
       `allow.txt`. If it doesn't, it is a divergence.
     - **follows N lines, from M**: a game lost lines. The only exception
       is a commit that turns a silent guess into a refusal, whose lost
       lines were followed by luck. The gate accepts it only where the
       game now stops at a refusal and `allow.txt` has `refusal <game>
       <the line that was wrong>`. Name the same in the commit body and
       the handoff. Any other loss is a regression to fix.
     - **newly presumes**: the game now relies on a card no one named in
       a graveyard having no ability there (`Game::presume_graveyard_inert`).
       Read the game and check that no card it could be acts from the
       graveyard. Then add `presumed <game> <why that is safe>`, or make
       the engine refuse there.
     - **no longer replayed**: Insight or the parser lost the game. Fix
       that.
     `allow.txt` is one line per allowance, `<kind> <game> <why>`, and the
     gate fails on a line without a reason. Copy every line of it into the
     handoff. The gate lists allowances that no game used. Delete those.
   - **Review.** Hand each rule's diff to one read-only sub-agent that has not
     seen your reasoning. Brief it with the diff only, not your argument, and
     have it check that each cited rule says what the code does (quoted from
     the pinned CR) and the card against Scryfall. Above all, it should look
     for a **silent guess**: a value the engine can't know, defaulted (a
     `filter_map` that drops unknowns, a `*` read as 0, a type assumed) where
     it should refuse by name. The replay can't catch these, because a guess
     that matches the log looks like progress. For a diff to `replay.rs`, it
     also asks whether the change can make a divergence look like another
     kind of stop, or drop a candidate without stopping by name: a fixed cap
     (`CHOSEN_NAMES`, `BLOCK_READINGS`, `BACKTRACKS`) or options deduplicated
     by card name. Batch 12's `refused_at` hid a divergence that way, and no
     review saw it. It also asks of each choice a heuristic orders or prunes
     from later lines: is the true answer still among the backtrack options
     whenever another way could explain those lines (batch 23 stopped trying
     "none" for a card shown in the chooser's next turn, which Sneak Attack
     or a reanimation also explains), and does a replay test cover it with
     every player who chooses (batch 23's Show and Tell judged the first of
     two players before the second's card had entered, and stopped a
     two-player game). And it checks refusals the other way: does one stop a
     game where the CR says the instruction does what it can and skips the
     rest (CR 608.2b's illegal target, CR 609.3)? Batch 25's "a sacrifice by
     no player" did, for a targeted player who had left. Fix what it finds before you push. If you leave something, put it at the top of the handoff.
   - **Revert check.** Before you commit a rule, reverse its `src/` change
     (`git diff -- crates/chancellor-of-the-spires/src > r.patch && git apply -R r.patch`),
     run its new tests and watch each fail, then `git apply r.patch`. A branch
     that no test fails without is untested, however it reads: test it or drop
     it. Batch 22's "in your graveyard" fallback and batch 21's empty up-to slot
     shipped that way. A rule that reads a phrase for every card that prints it
     also gets a negative test, where that phrase's filter refuses something
     (batch 27's "this or another" was tested on Ayara only, never on
     Arahbo, the First Fang's "nontoken Cat" seeing a token Cat enter). Reverting the rule
     can't show a filter that matches too much.
   - A gate you can't pass: drop or revert the commit that broke it and say so
     in the handoff.
4. **Commit and push.** One commit per rule, its subject a lowercase claim
   about the game, ending with the milestone, as the history does:
   `chancellor: <what the game now does>, so <card> <does what>; milestone 85 of 500, 46.10% of its lines`.
   Update the README's milestone paragraph ("Worked card by card from there…")
   with the new numbers and the rules you added, in the same commit. No AI
   attribution. Push only with `.claude/skills/chancellor-batch/push.sh`,
   never a bare `git push`. It rebases onto `origin/main` and pushes, or exits
   2 when what came in touches more than `.claude/` and Markdown (an Insight
   commit counts: Chancellor reads its trees). On 2, rerun the gate on the
   rebased tree, then run it again.
5. **Handoff.** Rewrite `<state>/handoff.md`: what you did, the measured
   numbers, what the top of the worklist now needs and anything you half
   understood. The next writer reads all of it.
6. **Summary.** End with the milestone before and after (games to the end,
   lines followed), the commits and what you left.

## Rules

- You are the only writer. Sub-agents are for reading and review, not for
  writing engine code.
- Never `pkill -f` or `pgrep -f ... | kill` a pattern: it matches your own
  shell's command line and kills it. Batch c28 did this twice, and the second
  time it killed a mutation loop before the loop restored `game.rs`. Find the
  PID in one call and kill it in the next. Wait for any background loop that
  restores files to finish before you start a gate.
- Never touch `milestones/replay-500.txt`, the CR pin or the cached logs.
- The logs are cached in `~/.cache/chancellor/pdlogs`; never commit them or
  anything quoting a player's name.
