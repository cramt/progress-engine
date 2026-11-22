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

## Steps

1. **Start.** `git pull --ff-only`, and stop if the tree is dirty or the pull
   refuses (report it and end the thread). Pick the batch number `n`: one more
   than the highest under `<state>/batches/`. Build with `cargo build --release
   -p chancellor-of-the-spires --bin chancellor-logs`, then write the baseline:
   `target/release/chancellor-logs replay --top 40 > <state>/batches/<n>/baseline.txt`.
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
   - A replay inference (which card a fetchland found, a choice the log never
     prints) goes in `replay.rs`, never in the engine.
   - Stop at about 15 commits or 3 hours, whichever comes first.
3. **Gate**, before each push:
   - `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`,
     `cargo test -p chancellor-of-the-spires`, and if you touched Insight,
     `cargo test -p vivisurgeons-insight` and `insight lint --counts` with no
     count above `crates/vivisurgeons-insight/tests/lint-baseline.txt`.
   - Rerun the replay into `<state>/batches/<n>/after.txt`. Against the
     baseline, the milestone's **games to the end** and **lines followed** must
     not fall, and **divergences** (the engine disagreeing with the log) must
     stay 0. **Gave up going back** (a disagreement met once the replay ran
     out of tries, earlier choices untried) must not rise without the game
     named in the handoff and shown to be the search, not the engine: force
     the finds it needed and watch it replay past the line. A divergence is a bug to fix before pushing: the engine's, or the
     replay's inference. One exception: a commit that turns a silent guess
     into a refusal may lower lines followed. The lines it gave up were
     followed by luck. Name the game and the line that was wrong in the
     commit body and the handoff.
   - **Review.** Hand each rule's diff to one read-only sub-agent that has not
     seen your reasoning. Brief it with the diff only, not your argument, and
     have it check that each cited rule says what the code does (quoted from
     the pinned CR) and the card against Scryfall. Above all, it should look
     for a **silent guess**: a value the engine can't know, defaulted (a
     `filter_map` that drops unknowns, a `*` read as 0, a type assumed) where
     it should refuse by name. The replay can't catch these, because a guess
     that matches the log looks like progress. Fix what it finds before you
     push. If you leave something, put it at the top of the handoff.
   - A gate you can't pass: drop or revert the commit that broke it and say so
     in the handoff.
4. **Commit and push.** One commit per rule, its subject a lowercase claim
   about the game, ending with the milestone, as the history does:
   `chancellor: <what the game now does>, so <card> <does what>; milestone 85 of 500, 46.10% of its lines`.
   Update the README's milestone paragraph ("Worked card by card from there…")
   with the new numbers and the rules you added, in the same commit. No AI
   attribution. Push to main with `git pull --rebase` first; if that brought
   in anything, rerun the gate before pushing.
5. **Handoff.** Rewrite `<state>/handoff.md`: what you did, the measured
   numbers, what the top of the worklist now needs and anything you half
   understood. The next writer reads all of it.
6. **Summary.** End with the milestone before and after (games to the end,
   lines followed), the commits and what you left.

## Rules

- You are the only writer. Sub-agents are for reading and review, not for
  writing engine code.
- Never touch `milestones/replay-500.txt`, the CR pin or the cached logs.
- The logs are cached in `~/.cache/chancellor/pdlogs`; never commit them or
  anything quoting a player's name.
