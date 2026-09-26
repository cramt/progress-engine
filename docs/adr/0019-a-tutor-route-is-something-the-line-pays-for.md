# A tutor route is something the line pays for: a cast may land an artifact, a play may declare its cost, and an activation is an entry in the line

Settled for [#80](https://github.com/cramt/progress-engine/issues/80). The Lantern north star is the owner's: by the end of turn 5, Lantern of Insight is on the battlefield by any route, and Rashmi and Ragavan has been cast, out of one budget (CONTEXT.md, *North star*). This ADR decides which of the deck's tutor routes the engine learns, how, and in what order. It stands on the commander cast from the command zone (#78) and lands paying only what they make (#82), both now shipped, and on rocks as sources the line cast ([ADR-0018](0018-rocks-and-dorks-are-sources-the-line-casts.md)). Digging (Sensei's Top, Search for Azcanta, Dakra Mystic) belongs to [ADR-0017](0017-a-spells-draw-is-a-deal-the-path-sizes.md) and is not priced here.

## What each route is worth

Measured with `docs/research/tutor-routes.py`, which lives outside `crates/` and reads nothing in them. It deals 4,000 shuffles of `decks/lantern.txt` per seat and asks, per deal, whether *some* line reaches the north star with a given set of routes switched on. Its pilot is clairvoyant about their own library, so its levels sit above what a declared line reads; every set is played on the same deals, and the differences are what this ADR uses. "Today" is the hard-cast Lantern plus the three routes that already ship as a cast fetch or a delayed fetch: Trinket Mage, Fabricate, Tezzeret, Cruel Captain, and Urza's Saga.

Rocks counted as ADR-0018 counts them; 4,000 deals per seat, seed 80. *Alone* is today plus that route; *last out* is everything minus it; *in order* is what it adds when the routes are added in the order this ADR builds them.

| Route, as `decks/index.jsonl` prints it | Play: alone / last out | In order, play / draw | Mechanism | Decision |
|---|---|---|---|---|
| Tezzeret the Seeker, `{3}{U}{U}`, −X: an artifact of mana value X or less onto the battlefield | +2.62 / 1.82 | +2.62 / +3.52 | cast fetch, to the battlefield | **build, 1st** |
| Dizzy Spell, transmute `{1}{U}{U}`: a card of mana value 1 to hand | +3.25 / 2.38 | +5.35 / +6.38, with Whir | cast fetch, declared cost | **build, 2nd** |
| Whir of Invention, `{X}{U}{U}{U}` at X = 1, improvise: onto the battlefield | +2.57 / 1.60 | (with Dizzy Spell) | declared cost, and 1st | **build, with 2nd** |
| Expedition Map, `{1}`, then `{2}`, `{T}`, sacrifice: a land to hand, for Urza's Saga | +3.67 / 2.85 | +3.25 / +3.52 | activation, then the Saga's delayed fetch | **build, 3rd** |
| Artificer's Intuition, `{1}{U}`, then `{U}` and discard an artifact card: mana value 1 or less to hand | +4.10 / 2.85 | +3.10 / +3.60 | activation, discard as a cost | **build, 4th** |
| Goblin Engineer, `{1}{R}`: an artifact to the graveyard; `{R}`, `{T}`, sacrifice an artifact: return one | +2.15 / 1.45 | +1.50 / +1.65 | four new concepts | refuse |
| Repurposing Bay, `{2}{U}`, then `{2}`, `{T}`, sacrifice another artifact: mana value one more, onto the battlefield | +1.27 / 0.70 | +0.73 / +1.03 | activation, sacrifice as a cost | refuse |
| Inventors' Fair, `{4}`, `{T}`, sacrifice, three artifacts: an artifact to hand | +0.35 / 0.22 | +0.22 / +0.43 | a land's activation, an artifact count | refuse |
| Trinket Mage, Fabricate, Tezzeret, Cruel Captain | last out 2.20, 2.80, 2.62 | | cast fetch to hand | ships |
| Urza's Saga | last out 5.45 | | delayed fetch | ships |

Levels, which are clairvoyant and so only upper bounds: today reads 24.23% on the play and 30.73% on the draw. The four built routes take it to **38.55% and 47.75%**, +14.3 and +17.0 points. Everything, the refused three included, reads 41.00% and 50.85%, so the refusals leave 2.45 and 3.10 points on the table. With lands only, as the engine counts mana today (an earlier revision of the same script, same deals), today reads 18.65% and everything 30.53% on the play; alone, Intuition (+3.42), the Map (+3.15) and Dizzy Spell (+2.45) still lead, the two battlefield tutors are worth +1.40 and +1.68, and the refused three +0.80, +0.72 and 0.

Oracle text was checked against `decks/index.jsonl` for every card. Inventors' Fair is in the list; Trophy Mage finds mana value 3 only and is out.

## The decision

**1. A cast may put a non-land onto the battlefield.** `fetch ... to = "battlefield"` on an `on = "cast"` effect is accepted when no entry of its priority matches a land, and refused as today when one does. The refusal was written for Rampant Growth, whose land's tapped-ness no tag carries, and an artifact has no such question. It is the mirror of the delayed fetch's rule, which already lands an artifact and refuses a land. A battlefield clause counts what a cast fetch delivers, as it counts what the line casts and what chapter III delivers. This is Tezzeret the Seeker, whose −X resolves at sorcery speed the turn he is cast for no mana, the same reading the file already takes of Cruel Captain's −3. It is a deterministic removal ([ADR-0012](0012-tutors-are-deterministic-removals.md)), so it costs no width, and the walk already moves the card; only the file boundary refuses it. HANDS.md hand 40.

**2. A play may declare its cost.** An `on = "cast"` effect may carry `cost = "{...}"`, and the line bills that instead of the printed mana cost, in the bill and in the pips the class is narrowed on. Dizzy Spell's transmute is `{1}{U}{U}` and discard, at sorcery speed; its printed `{U}` is a turn-1 tutor, which is the confident wrong number the engine gives today for the same effect with no `cost`. Whir of Invention's `{X}` stays refused in a `[casting]` entry unless the card's effect declares a cost, and then the X is the pilot's, stated and printed. A declared cost holding `{X}` or a hybrid symbol is refused as `can_cast` refuses it. A `cast` clause counts a transmutation as a casting, and the run says so beside the cost it billed. `cost` is a value, like `look` and `after`, not a priority: which card the line plays is still `[casting] prefer`, so this is no new policy language ([ADR-0004](0004-one-mechanism-for-pilot-policy.md)). Improvise stays unmodelled, as ADR-0018 names every cost reducer, so Whir is a floor. HANDS.md hand 41.

**3. An activation is an entry in the line.** ADR-0017 left activated abilities undecided and fixed only their shape: an entry in the one line, not a list of its own. This settles the tutoring half. An effect may be `on = "activate"` with a `cost`, a `fetch` and a `to`, and optionally `sacrifice = true`, meaning the activated permanent leaves play and is counted nowhere, as the Saga is.

- **The `[casting]` entry that names the card covers both payments.** Reached by the line, it first pays the activation of a copy the line already put into play, then casts a copy from hand. The line is read again from its top after each, as ADR-0017 and ADR-0018 already read it, so a Map cast this turn can be activated this turn if the pool still pays.
- **At most once per permanent per turn.** Every tutoring activation here taps its source.
- **After the land drop**, because that is where the line runs, **with one exception.** An activation whose fetch names a land that the `[land_drop]` list ranks above every land in hand is paid *before* the drop, out of the sources already in play, and the drop then reads what it fetched. That is Expedition Map cast on turn 2 and activated on turn 3 so the Saga is turn 3's land, and it is most of the Map: with activations only after the drop, the Map adds 1.42 points on the play instead of 3.67. The exception is a function of counts and two lists the file already declares, so it adds no policy; nothing else is paid before the drop, because a line read twice a turn would cast by a different order than the one declared.
- **Refused by name, for now:** an activation on a creature, because summoning sickness would have to be modelled (ADR-0018 models it for dorks, and no tutor here needs it), and an activation on a land, because Inventors' Fair's condition counts artifacts in play and nothing reads that.
- **Deterministic.** Which copies are in play and unactivated is a function of the path, like what was cast, so it branches nothing. The fetch is ADR-0012's removal.

This is Expedition Map for Urza's Saga, and it composes with the Saga's delayed effect unchanged. HANDS.md hand 42.

**4. Artificer's Intuition is an activation whose cost discards.** Once the third decision and ADR-0017's discard both exist, Intuition is `on = "activate"`, `cost = "{U}"`, and a discard of one artifact card chosen by the file's `[discard] prefer`, the one list ADR-0017 made for every claimant on the hand. No list of its own. It is built last of the four because it waits on two tickets, not because it is worth least: it adds 3.10 points on the play after the other three, as much as the Map.

## Refused, with the reason and the number

Each is refused by name at the file boundary, as today, and the message carries the number. None is refused as a model; each needs something no other route here needs, and is worth less than what it needs.

- **Goblin Engineer, 1.50 points.** The largest refusal. It needs four things nothing else here needs: a fetch whose destination is the graveyard, a return from the graveyard to the battlefield, summoning sickness on an activation, and a sacrifice of another permanent as a cost. The first may come with the Loam north star, which reads the graveyard; when it does, Engineer is worth re-costing.
- **Repurposing Bay, 0.73 points.** Its cost sacrifices *another* artifact and finds mana value one more than that artifact's. Which artifact goes is a new claimant on the battlefield, which would need its own declared priority as the hand's discard did, and the relation between what is sacrificed and what may be found is one no query expresses: a file that sacrificed a Talisman and fetched the Lantern would be a confident wrong number. In this deck the only mana value 0 artifacts are the four artifact lands, so every activation also costs a mana source.
- **Inventors' Fair, 0.22 points.** `{4}` beyond itself, three artifacts in play, and then `{1}` for the Lantern: out of reach by turn 5 without rocks, and nearly out of reach with them. Its condition counts artifacts on the battlefield, which nothing reads.
- **Trophy Mage.** It finds mana value exactly 3, so it cannot find the Lantern at all.
- **Digging** (Sensei's Divining Top, Search for Azcanta, Dakra Mystic) is ADR-0017's, not this ADR's.

## Order

Tickets 1 and 2 are independent of each other; Whir of Invention needs both. Ticket 3 needs 2's `cost`. Ticket 4 needs 3 and ADR-0017's discard. The north-star file is rewritten once 1 to 3, #78, #82 and ADR-0018's source ticket are in; 4 joins it when it lands.

## What it costs

**No width of its own.** Each route is a removal or a deterministic change of state, and adds no group beyond the card the line names. That card costs a group, as any `[casting]` entry does: adding Dizzy Spell, Tezzeret the Seeker and Expedition Map to a line that already casts the Lantern, the three hand tutors and plays the Saga first takes it from 22 groups and 277,368,474,240 compositions to 25 groups and 1,027,177,734,375 at turn 5 on the play (`gauntlet test` on `decks/lantern.txt`). Both are far over the ceiling and sampled, as the north star has been since it priced a cost; nothing here moves a question from exact to sampled.

**The sampler learns nothing separately.** It walks the same `Board` ([ADR-0013](0013-one-board-deterministic-parallelism.md)), so its agreement is asserted at the three levels of [ADR-0001](0001-exact-enumeration-with-a-sampling-oracle.md) and is not evidence on its own. The independent evidence is `checker/`, which learns each route from the Comprehensive Rules, README and HANDS.md, never from `crates/`: loyalty abilities at sorcery speed (CR 606), transmute (CR 702.53), activated-ability costs (CR 602.2, 118.3), and sacrifice as a cost.

**No committed number moves** when any of these ships, because no committed file declares such an effect; the before/after sweep across both decks and both seats has to show that. The numbers move when the north-star file adopts them, which is its own ticket with its own sweep.

## Considered Options

- **A separate `[activate]` priority.** Rejected. ADR-0017 fixed the shape, and a second list over one pool is the two-claimants problem the land drop taught.
- **Expedition Map as one `{3}` cast that fetches.** Rejected. It needs three mana by turn 2 for the Saga to be turn 3's land, which no land-only hand has, so it could never reach the turn-5 north star.
- **Expedition Map as a delayed effect with `after = 1`.** Rejected. An activation waits on payment, not on a count of turns.
- **All activations after the land drop, with no exception.** Rejected on measurement: it keeps 1.42 of the Map's 3.67 points.
- **The line read before and after the drop.** Rejected. A pass before the drop spends the old lands on whatever the list reaches first, so `[commander, Lantern]` on three lands would cast the Lantern before the drop and then be unable to cast the commander after it, which is not the line declared.
- **Deriving a transmute cost from oracle text.** Rejected for now, for [ADR-0006](0006-oracle-tags-are-fetched-and-dated.md)'s reason. The library may ship `cost` entries keyed by query later, as ADR-0018 ships `adds`.
