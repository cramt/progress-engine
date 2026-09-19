# Worked hands

Concrete opening hands and what the engine should say about them.

These exist because the interesting bugs in this tool are not crashes — they are
numbers that are wrong by a factor of six and look entirely reasonable in a
report. A hand small enough to check by hand is the only thing that catches
those.

Each one states the hand, what actually happens, and what a naive model says
instead. Where those differ, that difference is the test.

**Most of these are not answerable yet.** Each is marked with what it needs.
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
this turn. Surveil 1 on entry. Cast **zero** Opts.

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

*Needs #10.*

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
drawn rather than lands played. Every mana-relevant criterion in the repo has
this hole today, including the fixture's own "commander on turn 2".

*Needs #10 (gate). This one is cheap — lands in play is a function of the
checkpoint path the engine already walks.*

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

*Needs #10.*

### 7. Hallowed Fountain plus Island, needing `{W}{U}`

```
Hallowed Fountain
Island
(a {W}{U} spell)
```

**Can cast it**, on turn 2. Fountain pays `{W}`, Island pays `{U}`.

The pair with hand 6: same query counts, different answer. Whatever solves this
has to see the sources jointly.

*Needs #10.*

### 8. Hallowed Fountain's tapped-ness is a decision

> As this land enters, you may pay 2 life. If you don't, it enters tapped.

So "does this land enter tapped" is not a property of the card, and
`otag:tapland` membership does not settle it. It is a choice the pilot makes,
and a model has to state which choice it assumes.

Assume always-pay and you overstate any deck that cannot afford the life.
Assume never-pay and you understate every real shockland manabase.

Same shape as mulligan bottoming and selection routing: a decision that needs to
be declared in the file rather than assumed by the engine.

*Needs #10. Open: whether the default is pay, don't-pay, or refuse to guess.*

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

*Needs #17, #43. The zone half of the question is #40, which has shipped.*

### 10. The same hand, different question

If the criterion were *Loam in hand by turn 5*, the same surveil should keep
Loam on top instead, and it arrives in hand next turn.

Same card, same effect, opposite routing — because the routing is part of the
question, not part of the card.

*Needs #17. The zone half of the question is #40, which has shipped.*

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

*Needs #10, #40.*

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

When the features land, these become tests. Until then they are the
specification: if an implementation disagrees with a hand here, one of the two
is wrong and it is worth knowing which before shipping a percentage.

They are also the regression surface for the effect library (#43). A stdlib that
silently stops covering a card changes hand 1 from one Opt to zero, and nothing
else in the repository would notice.
