# Every pilot policy is one mechanism: a declared priority over queries

Mulligan bottoming, selection routing, the land drop, spell casting and tutor targets are all one construct: an ordered list of Scryfall queries, evaluated against counts. They are `[mulligan] bottom`, `to_graveyard`, `[land_drop] prefer`, `[casting] prefer` and `fetch`. The choice is then a deterministic function of the composition, which keeps the engine exact. Building a separate language per resource is how the policies would come to disagree with each other. **Adding a new policy language is the failure this ADR exists to prevent.**

Two deliberate variations exist, and each is printed by any run that relies on it:

- **A spell `[casting]` does not name is not cast, while an unnamed land is still played.** "Any other spell" would put every card's mana cost into the grouping.
- **A tie inside one mulligan `bottom` entry is settled at random and priced**, not settled by decklist order. An order does not commute with the per-class merging of [ADR-0007](0007-enumeration-is-sized-per-class-of-question.md), and a property test shows it.

See [VISION.md: One mechanism for selection](../../VISION.md#one-mechanism-for-selection), [#7](https://github.com/cramt/progress-engine/issues/7), [#17](https://github.com/cramt/progress-engine/issues/17), [#54](https://github.com/cramt/progress-engine/issues/54).
