# The enumeration is sized per class of question, and only proven narrowings apply

A criteria file is partitioned into classes of questions that read the same things. Each class is enumerated on the coarsest grouping and the fewest checkpoints that can answer it ([#31](https://github.com/cramt/progress-engine/issues/31)). A cost keys the manabase only on the colours it demands ([#55](https://github.com/cramt/progress-engine/issues/55)). This way, one hard question no longer forces every easy question beside it to be estimated. Each narrowing is asserted as a property (a coarser grouping is a marginal of the finer one), not assumed. Every run reports an `enumerations` block, so width figures can be reproduced rather than quoted.

**A narrowing that cannot be proven sound is not applied**, even where it would save orders of magnitude. Under a declared `[land_drop]` priority the colour merge is refused, because merging renumbers the decklist-order tie-break and would play a different land. That costs 41,250 against 1.2 billion compositions on `decks/lantern.txt`, tracked in [#56](https://github.com/cramt/progress-engine/issues/56).

## Considered Options

- **One grouping per file.** This was the prior behaviour.
- **Raising `MAX_PATHS`.** Rejected.

See [VISION.md: Mana](../../VISION.md#mana).
