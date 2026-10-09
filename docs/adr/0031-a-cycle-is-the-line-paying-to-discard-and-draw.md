# A cycle is the line paying to discard a card and draw, and the entry naming it never casts it

The Cid deck in [#136](https://github.com/cramt/progress-engine/issues/136) plays eighteen Cid, Timeless Artificer, whose text on Scryfall is *Cycling {W}{U} ({W}{U}, Discard this card: Draw a card.)* and *A deck can have any number of cards named Cid, Timeless Artificer.* Cycling Cids is how the deck fills its graveyard. Nothing could say it: `on = "activate"` pays for a permanent the line already put into play, and a `cost` on a cast still casts the card. So *three Cids in the graveyard by turn 5* read 0.11% on a deck that does it most games. [ADR-0017](0017-a-spells-draw-is-a-deal-the-path-sizes.md) named cycling among the activated abilities it left undecided, and fixed their shape: an entry in the one line, not a list of its own.

## The decision

**A seventh trigger, `on = "cycle"`, with a required `cost` and a required `draw`, and nothing else.** The cost is billed to the turn's pool in place of the printed cost, stage by stage as every entry is ([ADR-0018](0018-rocks-and-dorks-are-sources-the-line-casts.md)), and narrows the class on its pips as a declared cost does ([ADR-0019](0019-a-tutor-route-is-something-the-line-pays-for.md) §2). The card leaves the hand for the graveyard as the cost is paid (CR 702.29a), so `zone = "graveyard"` counts it from then on. The draw is a cast's draw: one sized gap, dealt where the line paid for it, and the line is read again from its top after it, so a Cid a cycle draws is cycled the same turn where the pool still pays. A cycled card was never cast.

**The `[casting]` entry naming a card whose effect is a cycle cycles it, and casts none of it.** The effect says what the entry means, exactly as an activation's effect makes the entry naming Expedition Map pay for the activation (ADR-0019 §3). So ordering cycling against casting is where the entry sits in `prefer`, and there is no second list and no new entry syntax ([ADR-0004](0004-one-mechanism-for-pilot-policy.md)): `['name:"Animate Dead"', 'name:"Cid, Timeless Artificer"']` casts Animate Dead whenever it has a Cid to return and spends what is left on cycling.

**A line that both casts and cycles one card is refused, by the question that would need it.** A `cast` clause whose query matches a card the line cycles is refused by name: its count is zero by construction, and a 0% beside a deck that casts Cid sometimes would read as measured. The remedy is in the message: ask `zone = "graveyard"`, or leave the cycle effect out to ask about casting. A cycle on a commander the line names is refused too, because a commander is cast from the command zone and cycling is paid from hand, so the entry would have no reading.

**Landcycling is refused.** `fetch` on a cycle is refused by name, pointing at #136. It is a cast fetch's shape paid from hand, and nothing here needs it yet.

**The standard library ships no cycle.** Whether a line cycles a card or casts it is the pilot's, and the cost is printed per card rather than keyed by a query.

## What it costs

Each cycle is one more one-card sized gap, so a turn that pays for two cycles deals two. On a 99-card list shaped like the Cid deck, 18 Plains, 18 Islands, 18 Cids and 45 blanks, *three Cids in the graveyard* on the play is exact by turn 4 (21.16%, 9.6 s) and passes the 5,000,000-leaf ceiling by turn 5, where it is sampled and labelled an estimate (28.11% ± 0.06, 90 s, 76 s of it counting leaves). That is ADR-0017's class, not a new cost: the same list with Cid read as a `{W}{U}` cantrip the line casts answers *three Cids cast by turn 4* with the same 21.16%, on the same tree.

## Considered Options

- **A distinct entry form**, such as `'cycle: name:"Cid"'` in `prefer`, so one line could cast some Cids and cycle others. Rejected for now: it is a grammar inside the query strings, which every other priority would then have to refuse, and the effect already says what the entry means for an activation. Casting some and cycling others would also need a rule for *which* copies, which is a policy nobody has asked for. The refusal names the gap.
- **A `[cycle] prefer` list.** Rejected: a second list over one pool is the two-claimants problem ADR-0019 rejected for activations.
- **Read the cycling cost from oracle text.** Rejected for [ADR-0006](0006-oracle-tags-are-fetched-and-dated.md)'s reason, as ADR-0019 rejected deriving a transmute cost.
- **Count a cycled card as cast.** Rejected: it was not cast, and a `cast` clause counting it would be the confident wrong number this tool exists to refuse.

## Consequences

- No committed number moves: no committed file declares a cycle. Neither north-star file does, and VISION.md's note that the Loam number leaves cycling out stays true of that file.
- HANDS.md hand 65 is the worked hand. The sampler walks the same `Board` and agrees, asserted in `cycling_agrees_with_the_exact_engine`; `checker/test_cycling.py` plays it from the README.
