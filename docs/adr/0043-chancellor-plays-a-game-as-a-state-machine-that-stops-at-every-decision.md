---
status: accepted
---

# Chancellor plays a game as a state machine that stops at every decision, and its first game is Loam goldfished

Chancellor of the Spires answers what each permanent is through the layers ([ADR-0037](0037-chancellor-lowers-insights-trees-into-its-own-effects-and-applies-them-layer-by-layer.md)), and nothing plays a game. The next step is the game: turns, zones, priority, the stack, mana, casting, resolution and state-based actions, with the cards Insight reads.

## The first game is `decks/loam.deck.toml`, goldfished

One player plays Loam, a Commander deck, against an opponent who does nothing, from a shuffled library and a kept hand to a turn limit. Insight reads all 90 of its cards and all 169 abilities (2026-11-01), against Lantern's 99 of 100, so every refusal on the way is the engine's and none the parser's. A goldfished game is the thing Gauntlet already counts, so it can later be a third oracle beside the exact engine and the sampler: a run that plays the deck by the rules and says how often it did a criterion by turn N.

Two Modern decks against each other would have been the smaller pool, and playing in Curator the shortest way to something on screen. Both were offered; the player picked their own deck. Commander's own rules (the command zone, commander tax, 40 life, CR 903) are therefore in the first milestone, not deferred.

## The game is a pure state machine that yields decisions

- `Game` is the whole state: zones, turn and step, priority, the stack, mana pools, life, the layer state of ADR-0037, and a seeded RNG, so a game replays from its seed and its answers.
- The engine runs until a player has to choose, and stops with a `Decision` that says who chooses and what the legal answers are: priority (pass, cast, activate, play a land), targets, modes, X, mana payment, attackers, blockers, discard to hand size, a mulligan. It never chooses for a player. `step(game, answer)` checks the answer against the legal ones and runs on.
- Whatever drives it, a test script, a goldfish pilot, Curator's UI, a search, does so through that one surface. A test is a list of answers and the state it ends in, like HANDS.md.
- Rules the CR writes as automatic (turn-based actions, state-based actions, CR 704, putting triggers on the stack, CR 603.3) are the engine's, never a decision.

## Card text runs through lowering, refused by name

Each effect Insight reads is lowered into an action of Chancellor's own before it runs, as continuous effects are lowered into `Op`s: a node with no lowering is refused by name, never skipped. The milestone is a number like the layers' coverage: how many of Loam's cards run whole, then a game that plays Loam to turn 10 with no refusal. The order is whatever Loam's goldfish needs next, card by card.

## The goldfish pilot's choices are Gauntlet's declared policy

A goldfish needs a pilot. When a goldfished game answers a Gauntlet question, which land to drop and which spell to cast must come from the policy the criteria file declares, the same declared priority over queries VISION.md settles, and the run prints it. A pilot with its own policy language is the failure VISION.md is written against. Until then the pilot is a test script, or the simplest legal answer, and says so.

## Considered options

- **The engine calls a player callback for each choice.** Rejected: a callback holds the game's stack frames, so a game can't be saved, replayed, forked for search or sent across the wasm boundary mid-turn. A returned `Decision` can be all four.
- **Ascent runs the game.** Rejected in the research already: Magic is Turing complete, and the game is the Rust state machine; ascent derives what is true now.
