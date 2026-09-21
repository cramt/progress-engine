# Worked hands

Concrete opening hands and what the engine should say about them.

These exist because the interesting bugs in this tool are not crashes — they are
numbers that are wrong by a factor of six and look entirely reasonable in a
report. A hand small enough to check by hand is the only thing that catches
those.

Each one states the hand, what actually happens, and what a naive model says
instead. Where those differ, that difference is the test.

**One of these is not answerable yet**, and it is marked with what it needs and
with the measurement that says why it was refused rather than estimated. That
is the point: they pin the semantics before the code exists, so that building
the feature cannot quietly redefine the question — and hands 1, 2 and 3 are the
worked case, written down as one Opt on turn 1 long before anything could say
so, and answered as one Opt on turn 1 when something finally could.

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

This is a test, as `crates/pe-cli/tests/fixtures/hand-4.criteria.toml` against a
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
`pe-criteria`, against hands written as seven-card libraries, where both answers
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

This is a test, as `crates/pe-cli/tests/fixtures/loam-yard.criteria.toml`, with
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

*Answerable today.* Asserted in `crates/pe-cli/tests/cli.rs` as the pair above,
and at the engine level in `pe-criteria` as this hand written as a seven-card
library, where the two priorities answer turn 1 with a flat yes and no.

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
11, 12, 13 and 14 already have, which is every one of them but hand 5. Until
then they are the specification: if an implementation disagrees with a hand
here, one of the two is wrong and it is worth knowing which before shipping a
percentage.

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
