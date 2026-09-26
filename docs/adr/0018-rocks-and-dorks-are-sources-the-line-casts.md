# A rock or a dork is a mana source only once the declared line has cast it

Only lands make mana today, so every mana number is a lower bound, and on an artifact
deck it is a low one
([#74](https://github.com/cramt/progress-engine/issues/74)). A non-land card
becomes a **mana source** in exactly one way: the `[casting]` line casts it, and
an `[[effect]]` matching it declares `adds = n`. From then on it adds *n* mana a
turn from the card's fetched `produces` palette. Nothing else about the model
changes:

- **Which rocks are cast is the line.** `[casting] prefer` names them, in the pilot's order, like any other spell. A rock the line does not name is never cast, so it never makes mana. A file with no `[casting]` counts no rock, and its `can_cast` gates are unchanged. There is no new policy language ([ADR-0004](0004-one-mechanism-for-pilot-policy.md)).
- **When the pool grows, the line is read again from the top.** "The first entry the pool can still pay for is cast" is re-read after every cast that added mana to this turn. So `[Mind Stone, Sol Ring]` off one Island casts both on turn 1: Mind Stone cannot be paid at first, Sol Ring can, and then Mind Stone can. The walk still ends, because every cast takes a card out of the hand.
- **Timing comes from the rules, not from a declaration.** A non-creature source adds mana on the turn it is cast. That mana pays only for spells cast **after** it in that turn's line, and never for the source itself. A creature source is summoning-sick (CR 302.6) and adds mana from the next turn on. A file can delay any source with `after = n`, the key delayed effects already use, which is how a rock that enters tapped is declared.
- **So the bill is no longer one matching.** Island, Sol Ring, Memory Lapse is the reason. Settled as one Hall's condition over `{1}` + `{1}{U}` against Island + `{C}{C}`, it pays: Island takes the `{U}` and Sol Ring's own mana pays for Sol Ring. That is a confident wrong number. The fix is to match each spell only against the sources that were already there when it was cast. That is a matching with nested supply, settled as a small flow, and it is needed only on a turn where the line cast a source that pays that same turn. Every other turn is the existing matching, with earlier rocks as extra untapped sources.
- **A gate beside a budget still asks what the line left**, and what the line left now includes rock mana it did not spend.

**The standard library declares the amounts, keyed by query.** Scryfall's
`produced_mana` gives a palette but no amount, and Sol Ring makes two.
[ADR-0008](0008-effect-library-keyed-by-query.md) already settles who declares
an amount: the tool does, not the user. Two entries do it, both requiring
`otag:mana-rock or otag:mana-dork` (two new standard tags, fetched and dated per
[ADR-0006](0006-oracle-tags-are-fetched-and-dated.md)). Both exclude anything
whose wording makes the mana conditional, delayed, or paid for with more than
a tap: "enters tapped", "doesn't untap", "Spend this mana only", "Activate
only", "could produce", `, {T}: Add`, "for each", `{X}`. The first entry is
`adds = 1` for `o:"{T}: Add"`, which on Scryfall matches 353 cards. The second
is `adds = 2` for `o:"{T}: Add {C}{C}."`, which matches 17. Measured on
Scryfall, 2026-09-26. Across both decks the two entries count Sol Ring, Mind
Stone, Arcane Signet, the three Talismans, Birds of Paradise and Elvish Mystic,
and nothing else. **A library entry may understate a card and must never
overstate one.** Gilded Lotus read as `adds = 1` is a lower bound. Worn Powerstone
read as untapped would not be one, so it is excluded.

**Two cards in these decks are deliberately not sources, and the run names
both.**

- **Fellwar Stone** makes "one mana of any color that a land an opponent controls could produce". The north star is reached with the pilot's own cards, and opponents neither help nor hinder. With no opponent, CR 106.7 says the stone makes nothing. So it is counted as nothing, not as colourless. Exotic Orchard is the same case in the Loam deck.
- **Lotus Cobra's** mana is a landfall trigger rather than a tap, so no `adds` reaches it. What it makes depends on the order of the land drop and the cast inside one turn, and the walk plays land and then spells. Modelling it is its own ticket.

Every line that casts a card with a non-empty `produces` and no `adds` prints
that card as *cast and counted as making no mana*, beside `assumed_tapped`. The
same applies to Malevolent Rumble's Spawn token and to a Treasure maker. **Cost
reducers** (improvise, affinity, convoke) are not modelled either. A spell with
one pays its printed cost and is named. Whir of Invention is already refused for
its `{X}`.

**A source's identity in the grouping is its cost plus what it adds**: the amount,
palette narrowed to the pips the class demands
([ADR-0007](0007-enumeration-is-sized-per-class-of-question.md)), and delay. The
tie rule inside an entry does not change: the cheaper cost first, then decklist
order. So the narrowing merges two sources in one entry only when nothing
differently seen sits between them in decklist order. That is the same adjacency
argument as [#56](https://github.com/cramt/progress-engine/issues/56). An
ordering keyed on what a source adds as a class sees it would make the line depend on
the question, so it was rejected.

## Considered Options

- **Rocks count toward every gate, cast or not.** Rejected. Which rocks were cast, and when, is the pilot's decision. A gate that assumed it would be the undeclared land-drop problem again, one resource over.
- **One joint matching over the summed bill, with rocks added to supply.** Rejected. It lets a rock pay for itself (the Memory Lapse hand).
- **Amounts read from oracle text by the engine.** Rejected. The amount is a declaration keyed by query, like `look = 1`. Oracle text is used only inside the standard library's queries, where it is visible, dated by the index and overridable last-wins.
- **Fellwar Stone as a colourless source.** Rejected. It is an overcount whenever no opponent has a coloured land, which is every turn-1 on the play.
- **Treat a dork like a rock.** Rejected. Summoning sickness is the rules, and it costs Loam a turn.

## Consequences

- **No committed number moves** when this ships. Neither north-star file declares a line that names a rock, and route B's line names only Trinket Mage and the Lantern. The before/after sweep has to show that. The numbers move when the north-star files add rocks to a line, which is a separate, justified change.
- **Width.** Every named source is a castable group. Measured today by naming each rock as its own entry, which is the split the grouping will make, on `decks/index.jsonl`:
  - Lantern cast plus `{1}` left at turn 5 goes from 4 groups and 30,720 compositions (exact) to 11 groups and 284,738,168 (sampled) with seven entries. With two entries, Sol Ring and then `t:artifact otag:ramp -t:land`, it is 6 groups and 1,026,432, still exact.
  - Loam cast plus `{G}` left at turn 5 goes from 6 groups and 1,026,432 to 7 groups and 4,120,116 (exact) with the dorks in one entry, or 8 groups and 14,057,472 (sampled) with two.
  - Both north stars already price three colours and are sampled either way: Lantern goes from 17 groups and 2.0×10¹⁰ to 24 groups and 6.8×10¹¹, and Loam goes from 17 groups to 19 and 6.3×10¹⁰.
  - So a pilot who wants exactness writes one entry per kind of source rather than one per card.
- **Estimated effect.** An outside Python sim dealt 40,000 games per seat from `decks/`, with a heuristic land drop and the rocks first in the line. *Rashmi and Ragavan cast by turn 5 from the command zone* goes from 41% to 55% on the play and from 47% to 61% on the draw. *Lantern cast and Rashmi cast by turn 5* goes from 3.6% to 5.1% on the play; route 1 is bottlenecked on drawing the Lantern, not on mana. For Loam, *Borborygmos cast by turn 5* goes from 32% to 38%, and *Loam cast and Borborygmos cast* goes from 1.4% to 2.0% on the play. The heuristic land drop reads 41% where the engine's clairvoyant gate reads 48.5% for the Rashmi cost alone, so the deltas are what to trust, not the levels.
- **The sampler learns nothing separately.** It walks the same `Board`, so it agrees by construction, and that agreement is not evidence. The independent evidence is `checker/`, which has to learn the same rules from the CR and this ADR, never from `crates/`.
- **#81 (Rashmi's Treasure) reuses the source.** It is a source created by a trigger, with `adds = 1`, the any-colour palette, and a one-shot flag. When a one-shot source is spent is #81's to decide.
- **#82 (lands pay only what they make)** can reuse the amount for a karoo's two mana, but its land-return cost stays #82's.
