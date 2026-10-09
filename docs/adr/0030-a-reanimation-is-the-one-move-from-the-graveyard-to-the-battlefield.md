# A reanimation is the one move from the graveyard to the battlefield, and the line waits for something to return

The Cid deck in [#140](https://github.com/cramt/progress-engine/issues/140) puts its Cids in the graveyard and then returns them with Fix What's Broken, Immortal Servitude, Angel of Glory's Rise or Animate Dead. A criterion could ask *Cids in the graveyard* and *a reanimation cast*, but not *Cids on the battlefield by turn 6*, because nothing moved a card from the graveyard to the battlefield except Lumra's `returns`, which was a key of the mill, returned lands only, and fired on any trigger but an upkeep.

## The decision

**One effect, `reanimate`, on a cast.** `reanimate = '<query>'` is which cards the card may return and `reanimate_count` is how many, `"all"` or a number. Both are the card's half and both are required. Where the count is a number, `reanimate_prefer` is the pilot's half: a declared priority over queries, read by the tutor's rule, the first tier holding a card and inside a tier the group the decklist named first ([ADR-0004](0004-one-mechanism-for-pilot-policy.md)). A card no tier names never comes back, so the list decides every choice and nothing branches: the path stays a function of counts ([ADR-0012](0012-tutors-are-deterministic-removals.md)). Lumra is `mill = 4`, `reanimate = "t:land"`, `reanimate_count = "all"`, and `returns` is gone, so there is one way onto the battlefield from the graveyard rather than two.

**Only your graveyard, and only a permanent card.** The cards a reanimation may return are its query held to the cards the CLI reads as returnable: a land, or a permanent card with no land on another face. A query matches any face, so `t:land` takes in Search for Azcanta by its back, and in the graveyard it is an enchantment card (CR 712.8a); nothing here can tell which face matched, so such a card never comes back. An instant or a sorcery never does. The opponents' graveyards are not modelled: Animate Dead and Reanimate can take from any graveyard, and that is a floor.

**It resolves after the cast's fetch, draw and mill, and before its discard.** Lumra returns the lands it has just milled. What comes back leaves the graveyard's count and joins the battlefield's from that moment, and leaves the library's count as a fetched card does, because it is neither in hand nor cast. A `cast` clause does not count it.

**What comes back was not cast.** It makes no mana, attacks for nothing and fires no trigger of its own. A land comes back tapped and pays from the next turn as a land a spell put down does ([ADR-0025](0025-a-land-a-spell-puts-down-is-tapped-and-pays-from-the-next-turn.md)), and fires a landfall as Lumra's always did. A reanimated rock or dork is not a source. Every number that could have read any of that is a floor, and the run says so.

**The line casts a reanimation only while the graveyard holds a card it would return.** Animate Dead and Reanimate target, and a spell with no legal target cannot be cast (CR 601.2c). Immortal Servitude targets nothing and could be cast into an empty graveyard; no pilot does, so it waits the same way. A reanimation that also mills is cast for the mill, as Lumra is, and never waits. So that the wait ends inside the turn that fills the graveyard, the line is read again from its top after a fetch puts cards in the graveyard, as it already was after a mill or a discard. Reading it again changes nothing on a line with no reanimation, because a spell the bill could not pay is not paid by a larger bill. Every run that casts one prints the rule.

**X is the pilot's, declared twice.** Immortal Servitude's X is a mana cost, so it is `cost = "{4}{W}{B}{B}"`, the declared cost of [ADR-0019](0019-a-tutor-route-is-something-the-line-pays-for.md), and the query says mana value 4. Fix What's Broken's X is life, which nothing here counts, so it is its printed `{2}{W}{B}` and the query alone. Nothing checks that the two Xs agree.

## Considered Options

- **Keep `returns` beside a new `reanimate`.** Rejected: two keys for one zone move, one of them restricted to lands for no reason but history.
- **Default the count to all.** Rejected: whether a card returns one or all is printed on it, and a default would be the tool reading the card for you.
- **A numbered reanimation with no priority returns nothing**, as a mill's `keep` with no `to_hand` keeps nothing. Rejected: Animate Dead must target, so "nothing" is not a line the pilot can play. It is refused, naming the key, as a forced discard with no `[discard]` list is.
- **Price the tie** among creatures the list does not name, as a discard does. Rejected for now: a creature nobody named is one nobody asked about.
- **Cast it whenever it is affordable.** Rejected: that is an illegal play for Animate Dead and a wasted one for Immortal Servitude, and it moves the battlefield count down by however often the line casts it before the graveyard fills.

## Consequences

- Angel of Glory's Rise exiles every Zombie before it returns the Humans. The exile is not modelled, so a Zombie the line put on the battlefield is still counted there.
- `checker/` plays a reanimation from the README: `checker/test_reanimation.py`. It cannot fetch to the graveyard, so its tests fill the graveyard with a mill.
- HANDS.md hand 64 is the worked hand. The sampler walks the same `Board` and agrees, asserted in `reanimation_agrees_with_the_exact_engine`.
