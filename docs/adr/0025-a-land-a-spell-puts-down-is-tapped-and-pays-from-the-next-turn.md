# A land a spell puts onto the battlefield is read as tapped, and pays from the next turn

Rampant Growth, Nature's Lore, Cultivate, Three Visits, Wood Elves and Solemn Simulacrum are the most common ramp in Commander, and until this they could not be written at all. `fetch ... to = "battlefield"` on a cast was refused whenever its priority could find a land ([ADR-0011](0011-pessimistic-tappedness-refuse-the-unknowable.md), [ADR-0019](0019-a-tutor-route-is-something-the-line-pays-for.md) §1), because whether the land enters tapped is a fact about the spell, tapped for Rampant Growth and untapped for Nature's Lore, and no tag carries it. So a question such as *the commander on turn 3 off a dork and Nature's Lore* had no honest answer, and agents reached for `adds = 1` on Wood Elves instead, which counted nothing.

## The decision

**A land a cast or an activation puts onto the battlefield is read as entering tapped.** It is on the battlefield at once: a battlefield clause counts it from that turn, and the library is one card smaller (a deterministic removal, [ADR-0012](0012-tutors-are-deterministic-removals.md)). It pays nothing the turn it arrives, because the line had already paid, and from the next turn on it is a land in play like any other, of the palette its card makes. That is the pessimistic half of ADR-0011, applied to the one thing the card data cannot say. It is exact for Rampant Growth, Cultivate and Solemn Simulacrum, and a floor for Nature's Lore, Three Visits and Wood Elves, whose land could also pay later that same turn. Every run that declares such a fetch says so beside the fetch's priority.

**It needs `[land_drop]` declared, and is refused by name without one.** Which lands are standing is what a declared drop records (`played_at`), and the mana reading of a run with no declared drop assumes whichever lands pay, so it has nowhere to count one more land. The refusal names the remedy, `[land_drop] prefer = ['t:land']`.

**A turn's bill is held to its drops plus the lands a spell put down before that turn.** This is the one change inside the walk. The count that rejects a bill before any matching was the drops alone, and that is what kept a land off a spell from ever paying. Lumra's returned lands go through the same door, so on a run with a declared drop they now pay from the turn after Lumra resolves, where they were a floor before. The matching itself was already right: it counts what is standing and takes out what arrived this turn.

Nothing new is declared: the spell is a `[casting]` entry and the land is a `fetch` priority, the shapes both already had ([ADR-0004](0004-one-mechanism-for-pilot-policy.md)).

## Considered Options

- **Declare the tapped-ness per effect** (`tapped = false` on Nature's Lore). Rejected for now. It is the honest end state, but an untapped land arriving mid-line pays only for what the line casts after it, which is ADR-0018's nested matching with a land in place of a rock. That is a change to the bill, not to the file boundary, and it moves no question this ADR was written for: a land off a turn-2 Lore pays on turn 3 either way.
- **Keep refusing.** Rejected. The refusal was protecting one turn's mana, and it cost every later turn, the battlefield count and the library thinning, all of which are known exactly.
- **Count it as an `adds = 1` source.** Rejected. A land is a land: it is counted on the battlefield, it can be found by a fetchland or discarded by Borborygmos, and a source is none of those.

## Consequences

- **A delayed fetch that finds a land stays refused.** Urza's Saga's chapter finds an artifact, and no card in either deck needs a land off a chapter.
- **No landfall fires for it.** A landfall counts the turn's drop and what a fetchland found, and a land arriving mid-line is not among them, as Lumra's are not. A floor for a deck pairing ramp with landfall.
- **The sampler learns nothing separately.** It walks the same `Board`, so it agrees by construction. `checker/` has not learned it: no committed criteria file declares a land-finding cast fetch. A file that does needs the checker taught from this ADR first.
- HANDS.md hand 61 is the worked hand, and `natures_lore_puts_a_forest_onto_the_battlefield_that_pays_from_the_next_turn` asserts it through the binary.
