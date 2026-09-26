# Worked hands

Concrete opening hands and what the engine should say about them.

These exist because the interesting bugs in this tool are not crashes — they are
numbers that are wrong by a factor of six and look entirely reasonable in a
report. A hand small enough to check by hand is the only thing that catches
those.

Each one states the hand, what actually happens, and what a naive model says
instead. Where those differ, that difference is the test.

**Some of these are not answerable yet**, and each is marked with what it
needs. Hand 5 also carries the measurement that says why it was refused rather
than estimated. Hands 19 to 25 pin what
[ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md) decided,
and hands 26 to 33 what
[ADR-0018](docs/adr/0018-rocks-and-dorks-are-sources-the-line-casts.md) decided,
before any of it is built. That is the point: they pin the semantics before the
code exists, so that building
the feature cannot quietly redefine the question — and hands 1, 2 and 3 are the
worked case, written down as one Opt on turn 1 long before anything could say
so, and answered as one Opt on turn 1 when something finally could.

Several of them are **pairs**, and that is deliberate rather than tidy. The
claim a hand makes is usually that one number moves while another does not, and
a test that ran only the half it expected to fail would pass against a model
that always says no.

## A caveat that applies throughout

The model sequences at sorcery speed. Opt is an instant and you would really
cast it on your opponent's end step; here it is cast on your own turn. That is a
simplification, it is stated rather than discovered, and it does not change any
of the counts below.

---

## Mana is a budget, not a gate

### 1. One Island, six Opt

```
Island
Opt ×6
```

**Turn 1:** play Island, tap it, cast one Opt. Scry 1, draw 1. Five Opts are
dead in hand.

**Naive model:** six Opts in hand means six filters and six draws. Overstates
the turn by six times.

The mana is spent by the first Opt. Holding a card is not casting it.

*Answerable*, as `cast`, and the hand is a test — `hand-1.txt` against
`six-opts.criteria.toml`, seven cards so that every path is this deal and the
answer is a yes or a no:

```toml
[casting]
prefer = ['name:"Opt"']

[[criterion]]
name = "an Opt cast on turn 1"
require = [{ turn = 1, cast = 'name:"Opt"', min = 1 }]
```

| | hand 1 | hand 2 | hand 3 |
|---|---|---|---|
| an Opt cast on turn 1 | **100%** | **0%** | **0%** |
| two Opts cast on turn 1 | 0% | 0% | 0% |
| six Opts in the opening hand | 100% | 100% | 100% |

The last row is the naive model's number, on the same run: the cards really are
all there, and holding them is what casting them is not.

**What this does not claim is the scry and the draw.** One Opt is cast; what
that Opt then *does* is the replacement-draw tier, and it is not built — see
hand 5. So the count of castings is exact and the cards it would have dug are
not modelled.

*Answerable as of #10's budget half. The draw is #57, which needs #18.*

### 2. One Undercity Sewers, six Opt

```
Undercity Sewers
Opt ×6
```

**Turn 1:** play Undercity Sewers. It **enters tapped**, so it makes no mana
this turn — which the gate now knows, from `otag:tapland`. Surveil 1 on entry.
Cast **zero** Opts.

Same shape as hand 1, one card different, and the answer changes from one Opt to
none. The filtering and the mana pull in opposite directions — you see one more
card and cast nothing.

This is the tradeoff a Lantern deck actually makes, and the tool should be able
to price it.

*Answerable*, and it is the second column of the table in hand 1 — the same
criteria file against `hand-2.txt`, which differs from `hand-1.txt` by one
card. The surveil still fires, from the standard effect library, on the land
drop that made no mana.

### 3. Seven Opt, no lands

```
Opt ×7
```

**Every turn:** cast nothing. There is no mana and never will be, because
nothing here draws or produces.

**Naive model:** seven cantrips, so seven cards deep. This hand does nothing at
all.

*Answerable*, and it was answerable as a gate before the budget existed:
`can_cast` is false on every turn, because no land was ever played. What is new
is that the same file now says it as a count — zero Opts cast, on every turn of
the run — which is the third column of hand 1's table and is the claim a reader
of "seven cantrips" would have got wrong.

---

## Land drops are use-it-or-lose-it

### 4. Five lands, two spells, on turn 3

```
Island ×5
Opt ×2
```

**Turn 3:** three lands in play, not five. One drop per turn.

**Naive model:** a clause reading `turn = 3, query = "t:land"` sees five, because
it counts cards
drawn rather than lands played. Every mana-relevant criterion in the repo had
this hole, including the fixture's own "commander on turn 2".

*Answerable*, as `zone = "battlefield"` on a query matching lands. It was cheap
exactly as predicted — lands in play is a function of the checkpoint path the
engine already walks, and it adds no group and no path.

```toml
[[criterion]]
name = "three lands in play by turn 3"
require = [{ turn = 3, query = "t:land", zone = "battlefield", min = 3, max = 3 }]
```

This is a test, as `crates/ichormoon-gauntlet/cli/tests/fixtures/hand-4.criteria.toml` against a
nine-card library that is entirely in hand by turn 3, so the two clauses read
100% and differ by two cards rather than by a probability.

**The fixture's "commander on turn 2" did not move, and it is worth knowing
why.** It asks for two lands by turn 2, and by turn 2 there have been two land
drops — so *two lands drawn* and *two lands played* are the same set of hands,
to the last digit. Every land clause in this repository asks for N lands by turn
T with N ≤ T, which is the only reason none of the recorded numbers changed. A
clause asking for three lands by turn 2 would have moved, and would have been
wrong before.

### 5. Opt draws a land

```
Island
Opt
(top of library: Island)
```

**Turn 1:** play Island, cast Opt, draw the second Island. The land drop is
already spent, so the drawn Island is **not** mana this turn.

**Turn 2:** play it. Two mana.

Drawing a land and being able to use it are one turn apart, and a model that
conflates them is optimistic in a way that compounds every turn.

**Half of it is answerable and the half that is not is the draw.** *The land
drop is already spent* is hand 4, and it ships. *Opt draws the second Island*
does not: the budget knows Opt was cast and does not model what casting it
then did.

*Needs the replacement-draw tier, which is refused rather than sampled, and the
reason is a measurement.* The enumeration reveals cards in library order, one
checkpoint each, and a card the walk might or might not draw needs a checkpoint
of its own — an unordered pair cannot say which of two revealed cards the draw
took. Each such checkpoint multiplies the enumeration by the number of groups,
and a turn with *T* mana can cast *T* cantrips, so the floor is one extra
checkpoint per turn. Measured off the group counts real runs report, on
`decks/lantern.txt` and `decks/loam.txt`, that floor is:

| Line | groups | exact to, today | exact to, with one replacement draw a turn |
|---|---|---|---|
| a `{1}` one-drop | 4 | turn 8 | turn **4** (turn 5 is 31,457,280, 6× the ceiling) |
| `{1}{G}{G}` | 6 | turn 5 | turn **2** (turn 3 is 6,158,592) |
| a two-spell line with a colour | 7 | turn 5 | turn **2** (turn 3 is 28,840,812) |

Both north stars ask about turn 5. So a replacement draw would be sampled on
every question this tool exists to answer, which is not a feature — it is a
percentage that changed kind. It is
[#57](https://github.com/cramt/progress-engine/issues/57), and what it needs is
[#18](https://github.com/cramt/progress-engine/issues/18)'s population that is
not fixed, not more checkpoints.

**Now decided, not yet built.**
[ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md) makes the
draw a gap whose size the path decides, which is the non-fixed population above
and not a checkpoint on every turn. Opt's scry deals one card. Kept on top, it
is the card the draw takes, so the draw deals nothing further. Sent to the
bottom, the draw deals one more. Either way the drawn Island waits for turn 2,
because a land drawn after the drop waits for the next one. The number this
hand asks for is the same as above: two lands in play on turn 2, and one on
turn 1.

*Not answerable yet: needs sized gaps and a bottom destination (ADR-0017).*

---

## Colours do not add up

### 6. One Hallowed Fountain, needing `{W}{U}`

```
Hallowed Fountain
(a {W}{U} spell)
```

**Cannot cast it.** One land, one mana, two pips.

**Naive model:** two clauses, `produces:w` at `min = 1` and `produces:u` at
`min = 1`, are both **satisfied**, because Hallowed Fountain counts toward both.
The conjunction is true and the spell is uncastable.

This is why castability has to be a primitive rather than something a user
assembles from counts. It is a bipartite matching — can these sources be
assigned to these pips — and no arithmetic over independent counts answers it.

*Answerable*, as `can_cast`:

```toml
[[criterion]]
name = "{W}{U} payable on turn 3"
require = [{ turn = 3, can_cast = "{W}{U}" }]
```

This is a test, and it is the same test as hand 7 — see there.

### 7. Hallowed Fountain plus Island, needing `{W}{U}`

```
Hallowed Fountain
Island
(a {W}{U} spell)
```

**Can cast it**, on turn 2. Fountain pays `{W}`, Island pays `{U}` — and the
Fountain has to be the turn-1 drop, because a shockland that arrives on the turn
you need it makes no mana (hand 8).

The pair with hand 6: same query counts, different answer. Whatever solves this
has to see the sources jointly.

*Answerable*, and **the pair is the test rather than either half of it**. It is
one criteria file — `hands-6-and-7.criteria.toml` — run against two decklists
that differ by one card:

| | `produces:w` ≥ 1 and `produces:u` ≥ 1 | `can_cast = "{W}{U}"` |
|---|---|---|
| hand 6: Fountain alone | 100% | **0%** |
| hand 7: Fountain and Island | 100% | **88.89%** |

The naive conjunction cannot tell the two hands apart and the gate answers them
oppositely, which is the whole claim. The 11% shortfall in hand 7 is one deal in
nine — the one where the Fountain is the card left out of the opening hand, so
it arrives on turn 3, has to be played on turn 3, and enters tapped under the
assumption in hand 8. The pair is also asserted at the engine level in
`gauntlet-criteria`, against hands written as seven-card libraries, where both answers
are a flat yes and no.

### 8. Hallowed Fountain's tapped-ness is a decision

> As this land enters, you may pay 2 life. If you don't, it enters tapped.

So "does this land enter tapped" is not a property of the card, and
`otag:tapland` membership does not settle it. It is a choice the pilot makes,
and a model has to state which choice it assumes.

Assume always-pay and you overstate any deck that cannot afford the life.
Assume never-pay and you understate every real shockland manabase.

Same shape as mulligan bottoming and selection routing: a decision that needs to
be declared in the file rather than assumed by the engine.

*Answered, and the data turned out to be better than this hand assumed.*
`otag:tapland` does not contain Hallowed Fountain at all — Scryfall keeps the
contingent ones in `otag:conditional-tapland`, 179 lands against the other tag's
495, overlapping by nine oddities. So the two tags separate *enters tapped* from
*offers you a decision* without any inference, and both are fetched at sync
time.

The decision is then made **pessimistically and out loud**: a conditional
tapland is assumed to enter tapped, and every run that priced mana names the
cards it assumed it about, on stderr and as `assumed_tapped` in the JSON. Never-
pay understates a real shockland manabase, which is the direction this tool
prefers to be wrong in — a number the deck can beat is a better failure than one
it cannot reach.

*Still open: making the choice declarable per file, which is what this hand
originally asked for and is the right end state. A default nobody stated is what
was refused; a default everybody is told about is what shipped.*

---

## Zones and routing

### 9. Loam into the graveyard, turn 1

```
Undercity Sewers
Life from the Loam
...
(top of library: Life from the Loam)
```

**Turn 1:** play Undercity Sewers. It enters tapped and surveils 1. The top card
is Loam — **bin it**. Loam is in the graveyard on turn 1.

This is the north star as a single hand, and it is the case that breaks a
keep-versus-discard framing. Binning the Loam is not a consolation prize for
failing to keep it; it is the deck working. The policy is a **router**, and the
criterion asks about the destination.

*Answerable today.* Written down, it is:

```toml
[[effect]]
match = "t:land otag:surveil"
look = 1
on = "landdrop"
to_graveyard = 'name:"Life from the Loam"'
```

The `match`, `look` and `on` come from the standard library and are repeated
here only because last-wins overrides a whole entry at a time. The
`to_graveyard` is the part the library will never declare.

This is a test, as `crates/ichormoon-gauntlet/cli/tests/fixtures/loam-yard.criteria.toml`, with
a ten-card cut-down beside it whose three zone counts are worked out on paper
in the test that reads them.

### 10. The same hand, different question

If the criterion were *Loam in hand by turn 5*, the same surveil should keep
Loam on top instead, and it arrives in hand next turn.

Same card, same effect, opposite routing — because the routing is part of the
question, not part of the card.

*Answerable today*, and it is the same file with the `to_graveyard` line taken
out. That is what the default means: no destination declared, so nothing leaves
the top, so the Loam arrives in hand on the following turn exactly as it would
have without the surveil.

The pair of these is asserted rather than described — the hand number moves
between the two files, which is the only thing that proves the routing is doing
work rather than being ignored.

### 34. Loam cast on turn 3

```
Jungle Hollow ×8
Life from the Loam
```

Nine cards, on the play, so every card is in hand by turn 3 and every deal is
this deal.

**Turn 1:** play Jungle Hollow. It enters tapped. **Turn 2:** play another; it
enters tapped too, so the only untapped land is turn 1's, one mana, and Loam
costs `{1}{G}`. **Turn 3:** two untapped green sources. Cast Loam. It is a
sorcery, so when it resolves it is put into its owner's graveyard. Loam is in
the graveyard by turn 3, and not by turn 2.

**Naive model:** casting is not a zone, so the cast Loam goes nowhere — out of
the hand, into nothing. The graveyard reads 0% on a hand that put the card
there, and the deck's only priced route to its north star counts for nothing
under the question it exists to answer.

### 35. The same Loam, no green

```
Tranquil Cove ×8
Life from the Loam
```

Hand 34 with every land swapped for one that makes white or blue. Same tapped
lands, same turns, and the `{G}` is never there. Loam is never cast, never in
the graveyard, and still in hand on turn 3.

*Answerable, and hands 34 and 35 are one test* — `hand-34.txt` and
`hand-35.txt` against `loam-cast.criteria.toml`, which declares the line and
asks the same clause of both:

```toml
[casting]
prefer = ['name:"Life from the Loam"']

[[criterion]]
name = "Loam in the graveyard by turn 3"
require = [{ turn = 3, query = 'name:"Life from the Loam"', zone = "graveyard", min = 1 }]
```

| | hand 34 | hand 35 |
|---|---|---|
| Loam in the graveyard by turn 3 | **100%** | **0%** |
| Loam in the graveyard by turn 2 | 0% | 0% |
| Loam cast by turn 3 | 100% | 0% |
| Loam in hand on turn 3 | 0% | 100% |

The pair is what makes the graveyard count one that moves with the casting: a
model that read the yard off the holding would say the same thing about both.
Both runs report the graveyard as reachable, because the line names a sorcery;
hand 35's zero is a measurement of a deck with no green, not a zone nothing
could reach. The same test runs the sampler on both hands.

A permanent the line casts is counted on the battlefield instead, never in
both. *Out of scope:* flashback and retrace, which cast a card **from** the
graveyard — a second casting this line does not make.

---

## Query semantics

### 11. Darksteel Citadel counted twice

```
Darksteel Citadel
```

Darksteel Citadel is an **Artifact Land**.

- `t:land or t:artifact` → **1**. Correct.
- `count('t:land') + count('t:artifact')` → **2**. A hand of one card reported as
  two.

The TOML schema takes one query per clause specifically so that the second form
cannot be written, and the combining happens in the query language, which gets
unions right.

*Answerable today for the first form. The second is unrepresentable, as of #44.*

---

## The Lantern north star

### 12. Surveil land plus Lantern

```
Undercity Sewers
Lantern of Insight
```

Lantern of Insight costs `{1}` — generic, so any land pays for it.

**Turn 1:** play Undercity Sewers. Enters tapped. Surveil 1. **Cannot cast
Lantern** — the only land is tapped.

**Turn 2:** the Sewers is untapped now. Cast Lantern.

So *Lantern in hand by turn 1* and *Lantern on the battlefield by turn 1* differ,
and the difference is a whole turn. This is the question VISION.md flags as not
yet settled: which one does the north star mean.

**Now add an Island to the hand, and the tool has to be told something it
cannot work out.** Both lands are a legal drop on turn 1, and they buy opposite
things: the Sewers digs a card and makes no mana this turn, the Island makes
mana and digs nothing. That is not a fact about the cards, it is a decision the
pilot makes, and it is the same shape as mulligan bottoming and selection
routing — so it is declared, in the file, as a priority over queries:

```toml
[land_drop]
prefer = ["otag:surveil"]        # or ["t:land -otag:tapland"]
```

*Answerable*, and **the pair is the test rather than either half of it**. One
deck — `hand-12.txt`, twelve cards: Undercity Sewers, Island, Lantern of
Insight and nine Lightning Bolt — and two criteria files identical but for that
list. Both hold the surveil's `to_graveyard` routing *and* `can_cast = "{1}"`,
which is the combination that was refused before
[#54](https://github.com/cramt/progress-engine/issues/54).

| | surveil first | untapped first |
|---|---|---|
| Lantern castable on turn 1 | **15.91%** | **31.82%** |
| Lantern castable on turn 2 | **62.05%** | 58.86% |
| a Bolt binned by turn 2 | 54.55% | 54.55% |

Turn 1 is worked on paper: seven of twelve cards are in the opener, so
"untapped first" wants the Island and the Lantern, which is C(10,5)/C(12,7) =
252/792, and "surveil first" wants the Island and the Lantern *and not the
Sewers*, which is C(9,5)/C(12,7) = 126/792 — exactly half, because the Sewers
takes the drop whenever it turns up.

**The reversal on turn 2 is the whole Lantern tradeoff, priced.** Playing the
tapland first costs the entire first turn and pays for itself immediately: the
surveil bins a Bolt before the turn-2 draw, so the draw digs a card deeper and
finds the Lantern more often. Filtering versus tempo is a number now instead of
an argument, and neither policy is the tool's opinion — both are the file's.

The binned Bolt does not move between the two, and that is the check that the
routing survived the arbitration: the surveil fires either way, one turn apart,
and by turn 2 it has looked either way.

*Answerable today.* Asserted in `crates/ichormoon-gauntlet/cli/tests/cli.rs` as the pair above,
and at the engine level in `gauntlet-criteria` as this hand written as a seven-card
library, where the two priorities answer turn 1 with a flat yes and no.

### 17. Urza's Saga, and the Lantern it goes and gets

```
Urza's Saga
Island
(library: Lantern of Insight)
```

**Turn 1:** play Urza's Saga as your land. It enters with a lore counter:
chapter I, and it taps for `{C}`.

**Turn 2:** after the draw step, a second counter. Chapter II. Play the Island.

**Turn 3:** after the draw step, a third counter, and chapter III resolves
**before you could play a land**: search the library for an artifact with mana
cost `{0}` or `{1}`, put it onto the battlefield. Lantern of Insight is on the
battlefield, having cost nothing. Then the Saga is sacrificed. You can tap it
for `{C}` with chapter III on the stack, and that mana stays in the pool for the
rest of the main phase, so turn 3 has the Saga's mana and turn 4 does not.

**Naive model:** Urza's Saga drawn by turn 3. That was what
`lantern.criteria.toml` wrote for a year, labelled an upper bound, because
chapter counters did not exist. It is wrong in the flattering direction: a
chapter III that looks for a Lantern you already drew finds nothing, and the
route it stood in for is not a route to anything new.

Three things have to be true of one hand, and each is a separate test:

- **When it fires.** Two turns after the drop, *after* that turn's draw. A
  Lantern drawn on turn 3 is not in the library for chapter III on turn 3.
- **What it moves.** The Lantern leaves the library and arrives on the
  battlefield **beside** the Saga — not in place of it, which is the
  difference from hand 16's fetchland.
- **What it costs.** The Saga is gone from the next turn on, and its mana with
  it.

*Answerable*, as an `[[effect]]` that waits:

```toml
[[effect]]
match = "name:\"Urza's Saga\""
on = "landdrop"
after = 2
sacrifice = true
fetch = ['name:"Lantern of Insight"']
to = "battlefield"
```

and **the number is checkable on paper**, which is the test. `hand-saga.txt`
is sixteen cards — the Saga, a Lantern, four Islands, ten Lightning Bolt —
with the Saga played the moment it is held:

| | play | draw |
|---|---|---|
| Urza's Saga drawn by turn 3 (the naive model) | 56.25% | 62.50% |
| Lantern on the battlefield by turn 3 | **20.42%** | **20.00%** |
| Lantern on the battlefield by turn 5 | **25.00%** | **23.75%** |
| Saga still on the battlefield on turn 5 | 12.50% | 12.50% |

On the play by turn 3 the Saga has to be in the opening seven and the Lantern
past the ninth card: 7/16 × 7/15 = 20.42%. By turn 5 two more drops can set it
up — the Saga eighth with the Lantern past the tenth, the Saga ninth with it
past the eleventh — for (49 + 6 + 5)/240 = 25.00%. The last row is the
sacrifice: the Saga is still there on turn 5 only if it was played on turn 4
or 5 — the tenth or eleventh card on the play, the eleventh or twelfth on the
draw — which is 2/16 in either seat.

**And it is the same number written without the effect.** The Saga in play by
turn *t* and the Lantern still in the library on turn *t*+2, for *t* = 1, 2, 3,
as three `any_of` branches: that is how `lantern.criteria.toml` asks it,
because declaring a land drop there would change what its other routes read.
The two phrasings agree to the last digit in both seats, on this hand and on
the real list, where the route reads **8.32%** on the play against the 9.09%
the naive model reported.

*Answerable today.* Asserted in `crates/ichormoon-gauntlet/cli/tests/cli.rs`
(the table, the agreement of the two phrasings, and the sampler), in
`gauntlet-criteria` as a ten-card library where the timing, the fetch and the
sacrifice are each a flat yes or no, and in `gauntlet-sim` against the exact
engine.

---

## The library is not a fixed population

### 15. Trinket Mage, and the Lantern is still in the deck

```
Island ×4
Trinket Mage
(library: Lantern of Insight)
```

**Turn 4:** four lands in play, `{2}{U}` paid, Trinket Mage resolves. It puts
Lantern of Insight from the library into your hand — and the library is now one
card smaller and holds no Lantern at all.

**Naive model:** you drew a tutor. A tutor is one card matching one query, and
whether it found anything is the reader's arithmetic rather than the tool's.
Every criteria file in this repository said so in a comment for a year.

The two things that move are not the same thing, and both have to. The Lantern
is **in hand**, which is the point of the card. The Lantern is **out of the
library**, which is the point of the engine: `for_each_checkpoint_path` computed
what remained as `groups - drawn`, so the population was a constant of the whole
path by construction.

*Answerable*, as an `[[effect]]` with a `fetch`, and **the pair is the test
rather than either half of it**. One deck — `hand-tutor.txt`, twelve cards: four
Islands, one Trinket Mage at `{2}{U}`, one Lantern of Insight at `{1}` and six
Lightning Bolt — and two criteria files identical but for the effect block:

```toml
[[effect]]
match = 'name:"Trinket Mage"'
on = "cast"
fetch = ['name:"Lantern of Insight"']
to = "hand"
```

| | drawn | fetched |
|---|---|---|
| Trinket Mage cast by turn 4 | **74.24%** | **74.24%** |
| Trinket Mage and a Lantern both cast by turn 4 | 59.09% | **71.82%** |
| Lantern still in the library on turn 4 | 16.67% | **1.52%** |

The first row is the one that must **not** move, and it is why this is a pair:
what a spell does when it resolves cannot change whether the pool paid for it. A
model that fired the tutor on the holding rather than on the casting would have
moved it.

The third row is worked on paper: turn 4 on the play has seen ten of the twelve
cards, so a named card is still in the library on 2/12 = 16.67% of deals. The
fetch empties all but the deals where the Mage was never cast.

On the real deck this is the Lantern north star's Route B, and it is the
difference between a route being priced and a route being answered: *Trinket
Mage and a Lantern both cast by turn 5* on `decks/lantern.txt` reads **0.78%**
without the fetch and **7.76%** with it. The first figure was the deck drawing
both halves naturally.

### 16. A fetchland is not a filter

```
Misty Rainforest
(library: basics)
```

**Turn 1:** play Misty Rainforest, crack it, and a basic arrives **in place of
it**. You have one land in play, as you would have had; the library has one
fewer land in it, which it would not have had.

**Naive model:** it looks at cards, so it is a scry or a surveil. It is not.
Scry and surveil examine N cards off the top and route them; a fetchland removes
a card from the library and shuffles. Modelling it as "look at N" produces
numbers that are wrong and plausible.

*Answerable*, and the pair again — `hand-fetchland.txt`, twelve cards: two Misty
Rainforest, four Island, one Forest, five Lightning Bolt:

| | as a land | fetching |
|---|---|---|
| a fetchland drawn by turn 3 | **95.45%** | **95.45%** |
| a fetchland on the battlefield by turn 3 | 95.45% | **18.48%** |
| a land in play on turn 1 | **100%** | **100%** |
| basics left in the library on turn 3 | 1.25 | **0.22** |

Three rows that must not move and one that must. A card is drawn when it is
drawn; a land drop still puts a land down; and the fetchland is *not there*
afterwards — except on the deals where the basics ran out, because a tutor that
finds nothing fetches nothing.

**And on a real deck the thinning is negligible.** `decks/loam-thinning.criteria.toml`
is this hand at scale, and the largest number it moves is *two Loam Access cards
by turn 8*, from 58.9176% to 59.0695% — one game in 658. Four fetchlands in 98
cards is 0.57 of one cracked by turn 8, each taking one land out of a library of
about ninety. The mean lands *drawn* falls at the same time, because the land
the fetch found is already in play and is one fewer left to draw. Thinning is
real, it is exact, and it is not why you play fetchlands.

**What is not answerable is the mana.** A Scalding Tarn fetches untapped and a
Terramorphic Expanse fetches tapped, `otag:fetchland` holds both, and nothing on
the land it found tells them apart — so a `can_cast` or a `cast` clause beside a
battlefield fetch is refused by name rather than answered optimistically.

---

## Discard, mill and dredge

None of these is answerable yet. Each pins a behaviour that
[ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md) decided,
using the Loam deck's own cards, with Beast Within as filler. All are on the
play. A resolved instant or sorcery goes to the graveyard
([#79](https://github.com/cramt/progress-engine/issues/79)), so each spell
below ends there too, and none of the counts below includes it unless a row
says so. Where a hand fixes the order of the library, it is a test at the
engine level with the deal written down, as hands 6 and 7 are.

### 19. Aftermath Analyst mills the Loam before you could draw it

```
Forest ×2
Aftermath Analyst
Beast Within ×4
(library, top first: Beast Within, Mountain, Life from the Loam, Island, Forest)

[casting]
prefer = ['name:"Aftermath Analyst"']
```

**Turn 2:** draw the Beast Within, play the second Forest, cast the Analyst,
and mill three: Mountain, Life from the Loam, Island. **Turn 3:** draw the
Forest, which is the card under the three, and play it.

**Naive model:** the Analyst is cast and does nothing, which is what the tool
says today. The Loam comes off the top on turn 4 and is **in hand**.

| | today | mill 3 |
|---|---|---|
| Aftermath Analyst cast by turn 2 | yes | yes |
| Loam in the graveyard by turn 4 | no | **yes**, from turn 2 |
| Loam in hand by turn 4 | yes | **no** |
| Forests on the battlefield on turn 3 | 2 | **3** |

The first row must not move: what a spell does cannot change whether it was
paid for. The last row is the reason a mill needs no checkpoint of its own. All
three cards are consumed, so nothing is left on top for the next draw to be
ambiguous about. The turn-3 draw is the fourth card, not the first.

**And a number on paper.** Take the same Analyst in a twelve-card library:
Forest ×2, Aftermath Analyst, Life from the Loam and Beast Within ×8. Of the
deals on which the Analyst is cast on turn 2, the mill takes the Loam on
**exactly one in three**. The cast needs only where the two Forests and the
Analyst fall. The other nine cards fill five seen slots, three milled and one
left over, and the Loam is equally likely to be in any of them. So the Loam is
in hand 5/9 of the time, milled 3/9 and still in the library 1/9.

*Not answerable yet: needs sized gaps and a compelled route on a cast
(ADR-0017).*

### 20. Malevolent Rumble keeps a permanent, and the Loam is not one

```
Forest ×2
Malevolent Rumble
Beast Within ×4
(library, top first: Beast Within, Life from the Loam, Mountain,
 Beast Within, Aftermath Analyst, Beast Within)
```

**Turn 2:** play the second Forest and cast Rumble. Reveal Life from the Loam,
Mountain, Beast Within, Aftermath Analyst. At most one permanent goes to hand
and **the rest go to the graveyard, because the card says so**. The file
chooses the permanent; the library does not.

| | nothing declared | `to_hand = ['t:land']` | `to_hand = ['name:"Life from the Loam"', 't:land']` |
|---|---|---|---|
| Loam in the graveyard on turn 2 | **yes** | **yes** | **yes** |
| revealed cards in the graveyard on turn 2 | 4 | 3 | 3 |
| lands on the battlefield on turn 3 | 2 | **3** | 3 |

Three claims, one per column:

- **An unrouted Rumble is not a no-op.** A surveil that routes nothing leaves
  every card on top. A Rumble that keeps nothing bins all four, because the
  destination is compelled rather than chosen.
- **The kept land is played.** It is in hand for turn 3's drop.
- **The list cannot take what the card cannot.** Life from the Loam is a
  sorcery, so an entry naming it matches nothing Rumble may keep, and the next
  entry decides.

The Eldrazi Spawn is not counted as mana, and a run that cast Rumble says so.

*Not answerable yet: needs sized gaps and hand as a chosen destination
(ADR-0017).*

### 21. Frantic Search, and the discard list decides where the Loam goes

```
Island ×3
Frantic Search
Beast Within ×3
(library, top first: Beast Within, Beast Within, Life from the Loam, Forest)

[casting]
prefer = ['name:"Frantic Search"']
```

**Turn 3:** the third Island makes `{2}{U}`. Cast Frantic Search, draw Life
from the Loam and the Forest, then discard two from a hand of seven: five
Beast Within, the Loam and the Forest. Untap the three Islands, and nothing is
left to spend them on.

| | `[discard] prefer = ['name:"Life from the Loam"', 't:land']` | `prefer = ['name:"Beast Within"']` | no `[discard]` |
|---|---|---|---|
| Frantic Search cast on turn 3 | yes | yes | refused |
| Loam in the graveyard on turn 3 | **yes** | no | refused |
| Loam in hand on turn 3 | no | **yes** | refused |
| lands in the graveyard on turn 3 | 1 | 0 | refused |

**Naive model:** Frantic Search digs two, so the Loam is found and in hand.
That is the middle column, and it is one declared list among several.

The third column is the claim that the tool never picks. A discard the card
forces is the pilot's choice. Without a list the run is refused, and the
refusal names `[discard] prefer` as the remedy, as a land drop with two
claimants and no `[land_drop]` is.

The middle column also shows why a tie costs nothing here. The two discards
come out of one entry that is one group, so there is one way to take them.

*Not answerable yet: needs sized gaps and `[discard]` (ADR-0017).*

### 22. Desperate Ravings discards at random, whatever the list says

```
Mountain ×2
Desperate Ravings
Beast Within ×4
(library, top first: Beast Within, Life from the Loam, Forest)
```

**Turn 2:** play the second Mountain and cast Desperate Ravings. Draw Life from
the Loam and the Forest, then discard one card **at random** from a hand of
seven.

| | `[discard] prefer = ['name:"Life from the Loam"']` | no `[discard]` |
|---|---|---|
| Loam in the graveyard on turn 2 | **1/7 = 14.29%** | **14.29%** |
| Loam in hand on turn 2 | 85.71% | 85.71% |
| a land in the graveyard on turn 2 | 14.29% | 14.29% |

The pair is identical, and that is the claim. The card chooses, so the list is
ignored rather than obeyed, and no list is not refused. The random discard is
priced as a branch over what the hand holds, three ways here, weighted 5/7,
1/7 and 1/7.

*Not answerable yet: needs sized gaps and a random discard (ADR-0017).*

### 23. Borborygmos and Fblthp cannot discard the Loam

```
(command zone: Borborygmos and Fblthp)
(on the battlefield after turn 5's drop: Forest ×2, Island ×2, Mountain)
(in hand: Life from the Loam, Mountain)
(library, top first: Forest)

[casting]
prefer = ['name:"Borborygmos and Fblthp"']
```

**Turn 5:** cast the commander for `{2}{G}{U}{R}` from the command zone. It
enters: draw the Forest, then you **may** discard any number of **land** cards.

| | `[discard] prefer = ['name:"Life from the Loam"', 't:land']` | `prefer = ['name:"Life from the Loam"']` | no `[discard]` |
|---|---|---|---|
| Borborygmos and Fblthp cast on turn 5 | yes | yes | yes |
| Loam in the graveyard on turn 5 | **no** | **no** | no |
| lands in the graveyard on turn 5 | **2** | 0 | 0 |
| Loam in hand on turn 5 | yes | yes | yes |

- **Which cards are eligible is the card's**, and the list cannot widen it. The
  Loam heads the list and stays in hand.
- **"Any number" is every held card the list names**: the Mountain and the
  Forest it just drew.
- **An optional discard with no list discards nothing**, and is not refused,
  where hand 21's forced one is.

*Not answerable yet: needs casting from the command zone
([#78](https://github.com/cramt/progress-engine/issues/78)), sized gaps and
`[discard]` (ADR-0017).*

### 24. A spell drawn mid-line is cast; a land drawn mid-line waits

```
(on the battlefield after turn 3's drop, which was an Island: Forest, Island ×2)
(in hand: Frantic Search, Beast Within ×2)
(library, top first: Life from the Loam, Forest)

[casting]
prefer = ['name:"Frantic Search"', 'name:"Life from the Loam"']
```

**Turn 3:** tap all three lands for Frantic Search. Draw Life from the Loam and
the Forest, discard two, and untap the three lands. The line is read again from
its top. If the Loam is still in hand, `{1}{G}` is there to cast it. The Forest
is not played: this turn's drop was the Island.

| | `[discard] prefer = ['name:"Beast Within"']` | `prefer = ['name:"Life from the Loam"', 'name:"Beast Within"']` |
|---|---|---|
| Frantic Search cast on turn 3 | yes | yes |
| Life from the Loam cast on turn 3 | **yes** | **no** |
| Loam in the graveyard on turn 3 | yes | yes |
| lands on the battlefield on turn 3 | **3** | **3** |
| Forest in hand on turn 3 | yes | yes |

The graveyard is the same and the route is not, which is why the north star is
asked of the graveyard and not of `cast`.

- **The first column is what the untap buys.** It is taken as untapping the
  three lands that paid for the spell, so the bill is where it was before the
  spell. That is a floor: a pilot could untap three better lands.
- **The fourth row is the stated floor on mid-line lands.** It is printed by
  any run that dealt a card mid-line.

*Not answerable yet: needs sized gaps, `[discard]` and a line that is re-read
after each resolution (ADR-0017).*

### 25. Dredge, and why a zone count is not an arrival

```
Forest ×2
Life from the Loam
...
```

**Turn 2:** cast Life from the Loam. It resolves, returns nothing, and goes to
the graveyard. The north star is met on turn 2.

**Turn 3:** the draw step could be replaced by Dredge 3: mill three and return
the Loam to hand.

- *Life from the Loam **put into** the graveyard by turn 5* is yes from turn 2,
  whatever happens next. This is the north star.
- *Life from the Loam **in** the graveyard on turn 5* is what a zone clause
  counts. It is yes today, because the tool never dredges. If the Loam
  dredged itself back on turn 3 and was not cast again, it would be **no**.

The two are the same number only while nothing takes a card back out of the
graveyard, and dredge is the first thing that would. Dredging the Loam is also
never a route *into* the graveyard, since it starts there. Shenanigans'
Dredge 1 is the one dredge that could mill the Loam, and in a scratch
simulation of the deck it moved nothing measurable.

*Not answerable yet, deliberately: needs a clause that counts arrivals, and a
dredge priority over queries against the graveyard (ADR-0017). Until then the
tool never dredges, which is a line the pilot could play, and it says so.*
## Rocks and dorks are mana the line cast

Hands 26 to 33 pin
[ADR-0018](docs/adr/0018-rocks-and-dorks-are-sources-the-line-casts.md) before
any of it is built. **None of them is answerable yet.** Each one needs the build
ticket named under it, which are the tickets #74 splits into. Until then, the
engine counts every rock and dork as a spell that costs mana and makes none, and
that is the "lands only" column in each table.

Every hand below is a seven-card library, played on the play, so the opening
hand is the whole deck and every answer is a yes or a no. "Filler" is any spell
the line does not name. The line is always written out: a rock the line does not
name is never cast, and so never makes mana.

### 26. Island, Sol Ring, Lantern of Insight

```
Island, Sol Ring, Lantern of Insight, filler ×4

[casting]
prefer = ['name:"Sol Ring"', 'name:"Lantern of Insight"']
```

**Turn 1:** play the Island and tap it for Sol Ring. Sol Ring is an artifact,
not a creature, so it taps at once for `{C}{C}`. One of those pays for the
Lantern. One `{C}` is left over.

| | the line above | the line reversed | lands only |
|---|---|---|---|
| Lantern cast on turn 1 | **100%** | 100% | 0% |
| Sol Ring cast on turn 1 | 100% | **0%** | 100% |
| `can_cast = "{1}"` on turn 1, beside the line | **100%** | 0% | 0% |

The reversed line, `[Lantern, Sol Ring]`, casts the Lantern off the Island and
then cannot pay for Sol Ring until turn 2. The order is the pilot's to declare,
and these two columns are the test that the engine reads it.

*Not answerable yet: needs #74's source ticket.*

### 27. Island, Sol Ring, Memory Lapse

```
Island, Sol Ring, Memory Lapse, filler ×4

[casting]
prefer = ['name:"Sol Ring"', 'name:"Memory Lapse"']
```

**Turn 1:** the Island pays for Sol Ring, and Sol Ring makes `{C}{C}`. Memory
Lapse costs `{1}{U}`, and no blue mana is left, so it is **not** cast. **Turn
2:** the Island untaps, so there is `{U}` plus `{C}{C}`, and Memory Lapse is
cast.

**Naive model:** add the two bills together, `{1}` plus `{1}{U}`, and match them
against Island plus `{C}{C}` in one go. That matching succeeds, because it lets
the Island pay the `{U}` and lets Sol Ring's own mana pay for Sol Ring. It says
turn 1, and it is wrong.

| | real | one joint matching |
|---|---|---|
| Memory Lapse cast on turn 1 | **0%** | 100% |
| Memory Lapse cast by turn 2 | 100% | 100% |

This is the pair to hand 26. The card count and the total mana are the same in
both hands; only the colour of the second spell differs, and so does the answer.
A rock's mana pays only for spells cast after it that turn.

*Not answerable yet: needs the source ticket.*

### 28. Island, Sol Ring, Mind Stone, with the rock you cannot afford listed first

```
Island, Sol Ring, Mind Stone, filler ×4

[casting]
prefer = ['name:"Mind Stone"', 'name:"Sol Ring"']
```

**Turn 1:** Mind Stone costs `{2}` and one Island cannot pay for it, so it is
skipped. Sol Ring can be paid, so it is cast. Now the pool has `{C}{C}` in it,
and the line is **read again from the top**: Mind Stone is paid for out of Sol
Ring's mana. Mind Stone then taps for its own `{C}`.

| | read again from the top | read once, top to bottom |
|---|---|---|
| Mind Stone cast on turn 1 | **100%** | 0% |
| `can_cast = "{1}"` on turn 1, beside the line | **100%** | 100% |

The rule is "the first entry the pool can still pay for is cast", and the pool
is still growing on turn 1.

*Not answerable yet: needs the source ticket.*

### 29. Rashmi off a Talisman, turn 3

```
Island, Forest, Forest, Talisman of Creativity, filler ×3
commander: Rashmi and Ragavan ({1}{G}{U}{R})

[casting]
prefer = ['name:"Talisman of Creativity"', 'name:"Rashmi and Ragavan"']
```

**Turn 2:** two lands pay for the Talisman. **Turn 3:** the third land comes
down. Now the Island pays `{U}`, a Forest pays `{G}`, the Talisman pays `{R}`
and the other Forest pays `{1}`. Rashmi is cast on turn 3, even though the deck
has no red land in it.

### 30. The same hand with Mind Stone

Swap the Talisman for Mind Stone and turn 3 still has four mana. None of it is
red, so Rashmi is never cast.

| | hand 29 (Talisman) | hand 30 (Mind Stone) |
|---|---|---|
| Rashmi cast by turn 5 | **100%** | **0%** |
| the rock cast on turn 2 | 100% | 100% |

**Naive model:** "a rock is one more mana". It says both hands cast Rashmi on
turn 3. The source's palette has to go into the matching, just as a land's does,
and this pair is hand 6 and hand 7 again, one card type over.

*Not answerable yet: needs the source ticket, and #78 for the commander.*

### 31. Elvish Mystic is summoning-sick

```
Forest ×3, Elvish Mystic, Life from the Loam, filler ×2

[casting]
prefer = ['name:"Elvish Mystic"', 'name:"Life from the Loam"']
```

**Turn 1:** the Forest pays for the Mystic. The Mystic is a creature and cannot
tap this turn. **Turn 2:** Forest, Forest and the Mystic make `{G}{G}{G}`, which
pays for Loam. So Loam is in the graveyard on turn 2, a turn earlier than lands
alone could put it there.

| | real | the Mystic taps like a rock | lands only |
|---|---|---|---|
| `can_cast = "{G}"` on turn 1, beside the line | **0%** | 100% | 0% |
| Loam cast by turn 2 | **100%** | 100% | 0% |
| Loam cast by turn 3 | 100% | 100% | 100% |

The first row is the claim. A dork adds nothing on the turn it arrives, and
treating it like a rock is the naive number.

*Not answerable yet: needs the source ticket.*

### 32. Lotus Cobra is not a source, and says so

```
Forest, Forest, Island, Mountain, Lotus Cobra, filler ×2
commander: Borborygmos and Fblthp ({2}{G}{U}{R})

[casting]
prefer = ['name:"Lotus Cobra"', 'name:"Borborygmos and Fblthp"']
```

**Real Magic:** Cobra comes down on turn 2. Each land after that adds one mana
of any colour through landfall. On turn 4, the four lands plus the landfall mana
make five, and Borborygmos is cast.

**Under ADR-0018:** Cobra is cast on turn 2, and it counts as making no mana.
There are never more than four lands, so the answer is **0%** by turn 5. The run
prints Lotus Cobra as *cast and counted as making no mana*. The number is a
lower bound, and the run names the card that makes it one.

*Answerable as that lower bound once the source ticket ships. The real turn-4 answer
needs a landfall ticket.*

### 33. Fellwar Stone makes nothing, because nobody else is at the table

```
Island, Island, Fellwar Stone, Trinket Mage, filler ×3

[casting]
prefer = ['name:"Fellwar Stone"', 'name:"Trinket Mage"']
```

**Turn 2:** both Islands pay for the Stone. It makes "one mana of any color that
a land an opponent controls could produce". A north star is reached with the
pilot's own cards alone (CONTEXT.md, *North star*). With no opponent, the Stone
makes nothing (CR 106.7). So there are only ever two mana, and Trinket Mage
(`{2}{U}`) is never cast. Swap in Mind Stone, and turn 3 has three mana and
casts it.

| | Fellwar Stone | Mind Stone |
|---|---|---|
| Trinket Mage cast by turn 5 | **0%** | **100%** |

The Fellwar column is a lower bound, and the run names the card. Reading the Stone as
colourless would overcount every turn 1 on the play.

*Not answerable yet: needs the source ticket (and the library-entry ticket for
the entry that leaves the Stone out).*

---

## Mulligans

### 18. Six lands and six spells, dealt until the rule is happy

```
Land ×6
Spell ×6

[mulligan]
keep = [{ query = "t:land", min = 2, max = 4 }]
bottom = ["t:land"]
down_to = 5
```

Not one hand but every hand this library can deal, because a mulligan is a
decision about which of them you play. Seven from twelve is 792 hands, and by
lands held:

| lands dealt | 0 | 1 | 2 | 3 | 4 | 5 | 6 |
|---|---|---|---|---|---|---|---|
| hands | 0 | 6 | 90 | 300 | 300 | 90 | 6 |

At seven the rule keeps two to four lands: 690 of 792. At six one land goes
back first, so the rule is asking for three to five *dealt* — 690 again, and a
different 690. At five the hand is kept whatever it holds.

**Turn 0 is the hand that was kept.** Asked *three or more lands in hand on turn
0*, the answer is

```
600/792 + (102/792)(390/792) + (102/792)(102/792)(96/792) = 0.8230…
```

— 600 of the sevens kept, 390 of the sixes kept with three lands left, and 96 of
the fives, which keep three or four lands only from a deal of five or six. The
naive reading, *three lands among the seven dealt*, is 696/792 = 0.8788, and it
is printed beside the answer rather than instead of it, because it is what
every number meant before a mulligan could be declared.

And the lands that went back are **in the library**, on the bottom of it: lands
in hand plus lands in the library is six on every deal.

*A test: `a_mulligan_on_a_two_group_deck_is_the_number_on_paper` and
`turn_zero_after_a_mulligan_is_the_hand_that_was_kept` in the engine suite.*

---

## Degenerate hands

### 13. Every card is a commander

**Refused**, not answered. The library is empty, every criterion would collect
zero probability mass, and the report would be a confident 0%.

*Answerable today. This is the existing `EmptyLibrary` refusal.*

### 14. A two-card library, drawing seven

**Refused** by both engines. Left alone they disagree by the whole answer:
enumeration finds no dealable hand and reports 0%, sampling deals what it can.

*Answerable today, and the symmetry is asserted — except for the empty-library
case in #37, which is the one hole.*

---

## What these are for

When the features land, these become tests — hands 1, 2, 3, 4, 6, 7, 8, 9, 10,
11, 12, 13, 14, 15, 16, 17, 18, 34 and 35 already have. That is every one of them
but hand 5, hands 19 to 25 (ADR 0017's tickets) and hands 26 to 33 (ADR 0018's).
Until then they are the specification: if an implementation disagrees with a
hand here, one of the two is wrong and it is worth knowing which before shipping
a percentage.

Hands 1, 2 and 3 are one test rather than three, for the reason hands 6 and 7
are: the claim is that one number moves while another does not, and a file that
only ran the hand it expected to fail would have passed against a model that
always says nothing was cast.

Hands 6 and 7 are the clearest argument for writing them down first. They are
one test rather than two, because either one alone proves nothing: the claim is
that the counts agree and the answer does not, and a test that only ran the
hand it expected to fail would have passed against a model that always says no.

They are also the regression surface for the effect library (#43). A stdlib that
silently stops covering a card changes hand 1 from one Opt to zero, and nothing
else in the repository would notice.
