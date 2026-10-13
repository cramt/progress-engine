# Chancellor of the Spires

A rules engine for Magic. It is a scaffold: nothing here plays a game yet.

[docs/research/rules-engine.md](../../docs/research/rules-engine.md) is the
plan, and the measurements that chose it:

- The game is a typed Rust state machine, because Magic is Turing complete
  and no fixpoint language can be the whole engine.
- What is true now, characteristics through the layers (CR 613) and
  legality, is [ascent](https://github.com/s-arash/ascent): Vivisurgeon's
  Insight's ability trees as facts under one fixed set of general rules,
  compiled in. Abilities are data, not rules per card.
- It runs in `wasm32-unknown-unknown`, so Meldweb Curator can carry it.
- The first slice is the 571 pinned Modern-metagame cards
  (`crates/vivisurgeons-insight/golden/modern-meta.txt`).
- Tests are rules questions with one right answer.

NAMES_FOR_FUTURE.md says why it is called this.
