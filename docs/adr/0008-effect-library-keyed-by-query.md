# The effect library is keyed by query, ships as a prelude, and never names destinations

Card data does not say what Opt does, and the user is not the one who should have to declare it. So the tool ships an effect library, and every run loads it first as a prelude, in the same `[[effect]]` syntax a user writes ([#43](https://github.com/cramt/progress-engine/issues/43)). The entries are keyed by Scryfall query rather than by card. That makes the library dozens of entries rather than thousands, and a new printing is covered the day it exists. When effects overlap on a card, the last one wins, so a user's file overrides the library with no special syntax.

The library declares what a card *looks at*, and **never where the looked-at cards go**. The destination is part of the question: the same surveil land wants Loam in the graveyard for one deck and on top of the library for another. A look that routes nothing is exactly a no-op, which is why autoloading the library moves no number. The library's hash is recorded in provenance.

See [VISION.md: Effects are declared per query, not per card](../../VISION.md#effects-are-declared-per-query-not-per-card).
