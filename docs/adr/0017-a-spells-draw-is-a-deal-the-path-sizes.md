# A spell's draw, look or mill is a deal the path sizes; discard is a declared priority; dredge waits

Supersedes the second half of [ADR-0010](0010-mana-gate-then-budget-replacement-draws-refused.md), the refusal of what a cast spell does when that is a draw. The gate and the budget stand. Settled for [#73](https://github.com/cramt/progress-engine/issues/73), on the costings in [docs/research/replacement-draws.md](../research/replacement-draws.md) and the Loam north star as [#72](https://github.com/cramt/progress-engine/issues/72) settled it: Life from the Loam put into the graveyard by turn 5 by any line the pilot's own cards allow, and Borborygmos and Fblthp cast, on one budget.

**The refusal was about the shape of the schedule, not the population.** ADR-0010 costed a draw as fixed one-card checkpoints paid on every turn whether the spell was cast or not, which is 1.96e19 compositions for a mill 3 on the real Loam class. Nothing about that representation is revisited: it stays rejected. What changes is that `chip-stats` stops requiring every gap to be fixed before the walk starts.

## The decision

**1. A gap's size may be decided by the path, as a removal is.** `chip-stats` gains one word, a **sized gap**: after each checkpoint the walk asks the caller how many cards the next gap deals, exactly where it already asks for removals. A gap of zero is one composition, so a path on which nothing fired pays nothing. The signature knows "the size of the next draw is a function of the path so far" and nothing about cantrips, which is the condition [#18](https://github.com/cramt/progress-engine/issues/18) set and ADR-0012 kept.

- **Every draw, look and mill that fires off a cast, an attack or a landfall is one sized gap**, and it is dealt as one **unordered block** when every card it reveals is consumed at once: to hand, to the graveyard, or split between them by a priority. Draw 2, mill 3, Malevolent Rumble's look 4 are one block each.
- **A look that leaves cards on top is dealt one card at a time**, because the next draw reads which card came first. Opt's scry is one singleton, and its draw is a second singleton only when the first went to the bottom; kept on top, the draw takes it and deals nothing.
- **The Board plays part of a turn.** Replaying a prefix, it stops at the first gap the path has not dealt and says how large it is. The line is read again from its top after each spell resolves, so a spell drawn mid-line is cast that turn if the pool still pays for it. **A land drawn mid-line waits for the next turn's drop**, even on a turn whose drop was not made: the drop comes before the line, as it does today. That is a stated floor, printed by every run that dealt a mid-line card, and a scratch simulation of `decks/loam.txt` puts it at about 0.05 points of the Loam north star.
- **The ceiling counts real leaves for a class with a sized gap.** The static product has no closed form for it, and its worst case refuses everything. Such a class is measured by a count of the paths the walk would take, capped at the ceiling. A class with no sized gap keeps today's bound, so nothing exact today changes kind.
- **The sampler deals the same gap in its true position**, asking the same Board the same question before each deal, as it already does for fetches. Agreement is asserted at the three levels of ADR-0001, and `checker/` replays each new route from its plain meaning.

**2. Mill is a compelled route, and the library may say so.** ADR-0008 keeps the library from naming a destination because the destination is the question. That reason covers a destination the pilot *chooses*. It does not cover one the card's text *compels*: Aftermath Analyst's three cards go to the graveyard whatever anyone asks, as do the three Malevolent Rumble does not keep. So the library states compelled destinations, and still never a chosen one. The chosen half is declared by the file on the effect, as a priority beside `to_graveyard`: one permanent to hand for Rumble and Midnight Tilling, lands to hand for Six, the bottom for a scry. An effect whose choice nobody declared keeps nothing to hand and bottoms nothing, which is what the card does when you choose nothing.

**3. Discard is a deterministic zone move, chosen by a declared priority.** Hand to graveyard, as `[discard] prefer = [...]`: a file-level list, because the hand is one resource that every outlet draws on, as the land drop is.

- **The library states what the card fixes**: how many, whether it is "any number", whether it is at random, and which cards are eligible. Borborygmos and Fblthp may discard only land cards, so no list makes it discard Life from the Loam.
- **"Any number" discards every held, eligible card the list names.** A forced discard takes the list in order.
- **A tie inside one entry, and the cards no entry names, are settled at random and priced**, as `[mulligan] bottom` settles them and for the same reason: a discard changes the hand that every question in the class reads, and an order does not commute with narrowing. It is a branch over the tier's composition. It is small when a tier is one group, and the file controls it by naming more entries.
- **A random discard ignores the list**: Desperate Ravings is priced over the whole hand.
- **A forced discard with no list declared is refused by name, with the remedy.** Which card goes is the pilot's, and ADR-0009's rule of no defaults applies.

This is the sixth resource on the one mechanism of ADR-0004, not a sixth language: an ordered list of queries, first match chosen, printed by every run that used it.

**4. A block nothing reads may be dealt last, over a coarser partition.** A shuffled library is exchangeable, so a block whose firing does not depend on its cards, and which nothing reads before the question, has the same joint law dealt at the end of the path as in place. Dealt there, it needs only the partition the question reads: Loam or not Loam. That is `chip-stats`' second word, a **tail**: a trailing gap dealt over a coarsening of the groups. It is Magic-free. It is a **narrowing** under ADR-0007, so it moves no number, applies only where its two conditions hold, and is asserted as a property: the tail is a marginal of the in-place block. The sampler never defers, so the two engines' agreement tests the exchangeability claim directly. Aftermath Analyst's mill is the case. A router, a discard, a dredge, or any clause reading the yard before the mill's turn is not deferred.

## What it costs, measured

Modelled with the research's path counter on the real 7-group class (`[casting]` one spell, then Life from the Loam), against 1,174,303 leaves today:

| route, on the play | turn 4 | turn 5 |
|---|---|---|
| a one-card cantrip, sized gap | — | 3,982,002, exact |
| draw 2 (Frantic Search's shape) | 1,764,096, exact | 11,356,235, sampled |
| look 4, all consumed (Rumble's shape) | 8,245,964, sampled | 53,109,836, sampled |
| Aftermath Analyst mill 3, in place | — | 26,468,358, sampled |
| Aftermath Analyst mill 3, as a Loam/not-Loam tail | — | **1,450,287, exact** |

**The union north star is sampled whatever is chosen here, and was before this ADR.** Every outlet the owner listed, as one `[casting]` line, is **32 groups and 13,233,297,555,456 compositions** at turn 5 on the play before a single card is drawn, milled or discarded (a `gauntlet test` run of that line on `decks/loam.txt`). So ADR-0010's objection, "sampled on every question it exists for", now reads the other way. Refusing the draw does not make the north star exact; it leaves it unanswerable. Sized gaps make each route exact to turn 4, and the Analyst's to turn 5 once it is a tail. The union is a labelled estimate under ADR-0003, from the same Board the exact engine walks.

## What stays out, and how each is labelled

- **Dredge.** Dredge is a *may*, so a run that never dredges plays a line the pilot could have played, and every number it gives is a floor rather than a wrong one. A run whose line can put a dredger in the graveyard says that it never dredged, the way it names lands assumed tapped. Life from the Loam's own Dredge 3 presupposes Loam is already in the graveyard, so it cannot move the north star. Shenanigans' Dredge 1 moved nothing measurable in a scratch simulation of the deck. In place it is the widest thing costed: 832,568,578 leaves, 709 times today. It also takes a card *out* of the yard, and the north star asks whether Loam was **put** there. A zone count and that arrival are the same number only while nothing leaves the graveyard. So dredge ships together with a clause that counts arrivals, and its choice is fixed now so that it cannot become a language of its own: *dredge or draw* is a declared priority over queries against the graveyard, read at each draw, and a file that declares none never dredges.
- **Activated abilities paid from the pool**: cycling, Glint-Horn Buccaneer's rummage, Sensei's Top, Dakra Mystic, Argoth. They are not decided here. When they come, an activation is an entry in the one line, not a list of its own.
- **Casting from the graveyard** (jump-start, flashback, retrace), and costs whose size the pilot picks (Nostalgic Dreams' X). They are low-value for the north star. Neither is refused as a model; neither is scheduled.
- **Mana a card makes in passing.** Frantic Search's untap is taken as untapping the three lands that paid for it, so it leaves the bill where it was. That is a floor. Rumble's Eldrazi Spawn is not counted. Both are named by any run that relied on them.
- **The fixed-slot representation** that ADR-0010 measured stays rejected.

## Considered Options

- **Fixed one-card checkpoints** (#57's shape). Rejected again: 1.96e19 by the bound.
- **Deferral alone.** Rejected as the mechanism, kept as a narrowing. It covers only blocks nothing reads, and every high-value outlet reads its block. Rumble keeps a permanent and Frantic Search discards from what it drew.
- **Sized gaps without the tail.** Rejected, because it leaves the one pure mill the owner named sampled on the play at the turn the north star asks about.
- **A per-effect discard list.** Rejected. The hand is one resource with many claimants, and the land drop showed what two opinions about one resource become.
- **Settling discard ties by decklist order.** Rejected for the reason ADR-0004 gives for `bottom`.

See [VISION.md: Mana](../../VISION.md#mana), [One mechanism for selection](../../VISION.md#one-mechanism-for-selection) and [HANDS.md](../../HANDS.md) hands 5 and 19–25.
