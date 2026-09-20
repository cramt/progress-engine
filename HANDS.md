# Worked hands

Concrete opening hands and what the engine should say about them.

These exist because the interesting bugs in this tool are not crashes — they are
numbers that are wrong by a factor of six and look entirely reasonable in a
report. A hand small enough to check by hand is the only thing that catches
those.

Each one states the hand, what actually happens, and what a naive model says
instead. Where those differ, that difference is the test.

**Several of these are not answerable yet.** Each is marked with what it needs.
That is the point: they pin the semantics before the code exists, so that
building the feature cannot quietly redefine the question.

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

*Needs #10 (budget), #43.*

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

*Needs #10, #17, #43.*

### 3. Seven Opt, no lands

```
Opt ×7
```

**Every turn:** cast nothing. There is no mana and never will be, because
nothing here draws or produces.

**Naive model:** seven cantrips, so seven cards deep. This hand does nothing at
all.

*Answerable as a gate*: `can_cast` is false on every turn, because no land was
ever played. How many Opts a hand with mana casts is still the budget, so the
hand above the line is answered and hands 1 and 2 are not.

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

*Needs #10, #43.*

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

*Half answerable.* The mana half is: `{ turn = 1, can_cast = "{1}" }` beside a
clause for the card in hand is the whole of "and one mana", and it reads false
on turn 1 and true on turn 2 exactly as above. What it cannot be written beside
is the surveil, because a routing effect and a mana question both decide which
land you played this turn, and a file holding both is refused rather than
arbitrated. So this hand's filtering and this hand's mana are each expressible
and not yet in one file.

*Needs #10 (the two policies reconciled).*

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

When the features land, these become tests — hands 4, 6, 7, 8, 9, 10, 11, 13 and
14 already have. Until then they are the specification: if an implementation
disagrees with a hand here, one of the two is wrong and it is worth knowing
which before shipping a percentage.

Hands 6 and 7 are the clearest argument for writing them down first. They are
one test rather than two, because either one alone proves nothing: the claim is
that the counts agree and the answer does not, and a test that only ran the
hand it expected to fail would have passed against a model that always says no.

They are also the regression surface for the effect library (#43). A stdlib that
silently stops covering a card changes hand 1 from one Opt to zero, and nothing
else in the repository would notice.
