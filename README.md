# progress-engine

Draw-probability tests for Magic: The Gathering decklists.

Named for Jin-Gitaxias's faction of New Phyrexia, on the grounds that obsessively
recalculating whether your deck is perfect yet is blue-aligned behaviour.

This README is what the tool does today. [VISION.md](VISION.md) is what it is
for, what it refuses to become, and which of those two lists a given decision
came from. [HANDS.md](HANDS.md) is a set of seven-card hands with the answers
written down — the specification, in the only form small enough to check by
hand.

## Why

Ask a simple question about a deck — *"how often do I have a white source in my opening
hand?"* — and you can get an answer that is confidently wrong in two different ways.

**Miscategorised cards.** In the session that produced this tool, Kor Haven was counted as a
white source because a regex matched the `{W}` in its *activation cost* rather than its mana
production. Thaumatic Compass was counted as a land when it is an artifact until you control
seven lands. Both errors moved a headline number, and neither announced itself.

**Unstated definitions.** The same deck was asked "what are the odds I have an evasive
connector and a way to arm my commander by turn 5?" and answered **31.0%** and **39.0%** — a
7.5-point spread that came entirely from one analysis counting 8 cards as "connectors" and
another counting 12. Both were defensible. Neither was written down.

The fix is to stop keeping these definitions in your head. Write them as queries, write the
thresholds you care about as assertions, and re-run them when the deck changes:

```toml
[[criterion]]
name = "t1 dork into t2 commander"
at_least = 0.55
require = [
  { turn = 1, query = "t:land", min = 1 },
  { turn = 1, query = 'cat:"Mana Dork"', min = 1 },
  { turn = 2, query = "t:land", min = 2 },
]
```

That is a unit test for a deck. Change a card, run it again, see which assertions moved.

## How it works

Probabilities are **exact**, not simulated. Cards are grouped by which of your queries they
match, and the tool enumerates count-vectors over those groups, weighting each by its
multivariate hypergeometric probability. There is no shuffler, so there is no sampler bias to
chase — and it is fast enough that the answer is instant. A question too wide to enumerate is
answered by sampling and [says so in capital letters](#when-the-question-is-too-wide);
everything else is exact. Enumerating rather
than sampling also means a count's whole *distribution* falls out of the same
walk instead of having to be estimated: see [how many, not just how
often](#how-many-not-just-how-often).

A clause asks only *how many* cards matching a query you have seen by a turn, never which
ones. That constraint is what keeps the engine exact: a criterion is a pure function of how
many cards of each query you drew, so it is evaluated once per possible composition instead of
once per simulated hand.

**The criteria file is data, not a program.** A `[[criterion]]` is a name, an optional
threshold, and a list of clauses that are ANDed; a clause is a turn, a query and a range of
counts. There is no expression language, because the boolean structure a criterion actually
needs already exists in the Scryfall query — `or`, `-` and parentheses — and putting it there
means one grammar instead of two. So the queries a file asks about and how far into the game
it looks are both readable without running it, which is what lets a refusal say what was
asked, and what makes hashing the file into the `provenance` block a promise that holds: the
file cannot behave differently the second time it is read.

`--simulate` is not a second feature set — it is a second implementation. It shuffles and
deals where the default enumerates, and wherever both can answer they must agree, which is
asserted at three levels: unit, through the criteria layer both engines share, and end to end
through the binary. Two independent routes to the same number is how a mistake in either one
gets caught rather than believed, and that is reason enough for the crate to exist. It stays a
flag: it is how you ask for sampling on a question that did not need it.

What `--simulate` is *not* is a way to inspect individual cards. There is no host language, so
there is nothing for a card-level criterion to be written in: a clause names a query and a
count and that is the whole vocabulary, in both engines. Anything that turns on *which* cards —
a specific interaction, an ordering, the best card in hand — cannot be asked here at all, and
`--simulate` does not change that. The wanted feature is
[issue #11](https://github.com/cramt/progress-engine/issues/11); describing it here as though
it shipped would be this tool's own defining failure mode aimed at its documentation.

### When the question is too wide

Compositions multiply with the number of groups the queries split the library into and with
how deep the turns go, so a criterion joining seven queries at turn six is a few billion of
them, against a ceiling of five million. That question used to be refused. It is now
**answered by sampling, loudly**:

```
$ progress-engine test simple-ramp.txt too-wide.criteria.toml
ESTIMATE: this question was too wide to enumerate exactly: 7918829568 compositions
          across 12 groups, against a ceiling of 5000000. It was answered by
          sampling 200000 hands instead.
          Every percentage below is an ESTIMATE, not an exact answer. The ±
          beside each one is its standard error, and a difference smaller
          than that is not a difference.
          Pass --exact to refuse a question this wide rather than estimate it.
PASS everything at once by turn 6   39.39% ± 0.11  (needs 10.0%)
     a land by turn 6               99.70% ± 0.01
```

The loudness is the whole safety argument, and it is why this is acceptable at all. A silent
fallback would be a number quietly changing kind — an estimate wearing an exact answer's
clothes — which is the failure this repository is named against. So the warning is the first
thing on stderr, every sampled figure is quoted with its error bar wherever it appears, and
the JSON says `"method": "sampled"` with `"sampled_because": "too_wide"` and the width it
refused beside it. A labelled estimate is a different answer to a question that was honestly
too big ([#48](https://github.com/cramt/progress-engine/issues/48)).

The refusal is still there under `--exact`, for anyone who would rather have no answer than an
approximate one — CI, and the cross-engine agreement tests, which need an oracle that either
enumerates or says nothing:

```
$ progress-engine test simple-ramp.txt too-wide.criteria.toml --exact
Error: this question is too wide to answer exactly: 7918829568 compositions across 12 groups.
It asks about 7 queries: t:land, cat:"Ramp", cat:"Ramp - One Mana", cat:"Draw", cat:"Ramp - Engine", t:creature, t:instant
Reduce the number of distinct queries, or ask about an earlier turn.
```

The refusal names all seven, because all seven are in the file and the file is data. It used
to name five, and six groups: queries were learned by running the JavaScript, so they arrived
a few at a time, the estimate went over the ceiling part-way through, and the last two were
never reached. The number reported what the run had found out before it gave up rather than
what had been asked ([#36](https://github.com/cramt/progress-engine/issues/36)).

**A threshold inside the error bar is not a verdict.** A criterion at 79.9% ± 0.3 against a
threshold of 80% has not really passed or failed, and rounding it one way without saying so
would be a coin toss wearing a verdict's clothes. The comparison is still made — a rule that
sometimes declines to answer is harder to build on than one that is always reproducible — but
when the threshold sits within two standard errors of the figure, the run flags it as its own
note and the JSON carries `"inconclusive": true`.

Falling back makes the tool usable at the ceiling. It does not raise it, and every feature on
the roadmap pushes width up, so the cheapest sufficient enumeration per criterion
([#31](https://github.com/cramt/progress-engine/issues/31)) still matters.

## Status

Working today:

| Command | What it does |
|---|---|
| `progress-engine sync` | Build the card index from Scryfall bulk data |
| `progress-engine parse <deck>` | The canonical Archidekt decklist parser, as JSON |
| `progress-engine test <deck> <criteria.toml>` | Evaluate criteria and report PASS/FAIL |

```
$ progress-engine test simple-ramp.txt simple-ramp.criteria.toml
PASS keepable opener (2-5 lands)   78.97%  (needs 70.0%)
PASS turn-1 accelerant             51.04%  (needs 35.0%)
PASS commander on turn 2           44.29%  (needs 30.0%)
     any ramp by turn 3            94.39%
     lands in opener              mean 2.55
                                  0: 3.7%   1: 16.4%  2: 29.7%  3: 28.6%
                                  4: 15.7%  5: 4.9%   6: 0.8%   7: 0.1%
     ramp seen by turn 3          mean 2.36
                                  0: 5.6%   1: 20.2%  2: 30.6%  3: 25.6%
                                  4: 13.0%  5: 4.1%   6: 0.8%   7: 0.1%

PASS: 3 of 3 assertions met
```

JSON goes to stdout, the verdict to stderr, and the exit code reflects it — so a
caller piping stdout through `jq` cannot lose the failure.

A criterion with no `at_least` is informational: it reports a number and cannot
fail. The two blocks with means under them are `[[expect]]` rather than
`[[criterion]]`, and they are the subject of [how many, not just how
often](#how-many-not-just-how-often). Add `--draw` to model being on the draw,
`--simulate` to sample instead of enumerate (slower, approximate, and reported
with standard errors), and `--exact` to refuse a question too wide to enumerate
rather than [estimating it](#when-the-question-is-too-wide).

The JSON also carries a `provenance` block — the tool's version, the date the
card index was built, and SHA-256 hashes of the decklist, the criteria file and
the [standard effect library](#effects) as they were read. The last of those is
the one nobody edited: it autoloads, so without it a percentage that changed
because the shipped library changed would be indistinguishable from one that
changed because your deck did. A percentage that moved since last week is useless on its own,
because your deck, your criteria, the index and the tool itself each move it and
in the output they are indistinguishable: a number changed. Naming the inputs is
what lets a comparison say *which* one changed rather than only that something
did. An index that never recorded when it was built reports `null` there, for the
same reason a card with no legality word comes back unknown: a plausible date
nobody can vouch for is worse than an admitted gap.

### The criteria file

TOML, and the whole schema fits in one example. A `[[criterion]]` has a `name`,
an optional `at_least`, and `require`: a list of clauses, all of which must
hold. A clause is a `turn`, a `query`, an optional `zone`, and at least one of
`min` and `max`. A criterion can also hold `any_of`, a list of alternative
routes, for which see [Routes](#routes-any_of). An `[[expect]]` is a `name`, a
`turn`, a `query` and an optional `zone`, and reports a distribution rather
than a verdict.

```toml
[[criterion]]
name = "keepable opener (2-5 lands)"
at_least = 0.70
require = [{ turn = 0, query = "t:land", min = 2, max = 5 }]

[[criterion]]
name = "commander on turn 2"
at_least = 0.30

  [[criterion.require]]
  turn = 1
  query = 'cat:"Ramp - One Mana"'
  min = 1

  [[criterion.require]]
  turn = 2
  query = "t:land"
  min = 2

[[expect]]
name = "lands in opener"
turn = 0
query = "t:land"
```

Both spellings of `require` above are the same TOML document, which matters
because the text format is an API rather than a user interface: a generator
emits inline tables, a person writes the expanded form, and neither has to
convert. `turn` counts from 0, the opening hand, and is cumulative — turn 3 is
everything seen by turn 3, not what arrived on it. On the play turn 1 draws
nothing, so turns 0 and 1 see the same seven cards.

**One query per clause, deliberately.** There is no way to add two counts
together, because `count(a) + count(b)` counts a card matching both twice —
`t:land` plus `t:artifact` reports Darksteel Citadel as two cards — where
`t:land or t:artifact` gets the union right. The combining belongs in the query
language, which already has `or`, `-` and parentheses and already handles
overlap. Making the footgun unrepresentable beats documenting it.

#### Routes: `any_of`

`require` is a conjunction, and some questions are a disjunction of them. The
alternatives are **routes to the same outcome** rather than alternative cards —
they differ in turn, in zone and in what they need, so no card query expresses
them and `t:land or t:artifact` is the wrong tool:

```toml
[[criterion]]
name = "a lantern by turn 5"
at_least = 0.80

  [[criterion.any_of]]
  require = [{ turn = 5, query = 'name:"Lantern of Insight"', min = 1 }]

  [[criterion.any_of]]
  require = [{ turn = 4, query = 'name:"Trinket Mage"', min = 1 }]

  [[criterion.any_of]]
  require = [{ turn = 3, query = "name:\"Urza's Saga\"", min = 1 }]
```

The criterion holds when **any** branch holds, and a branch holds when **all**
its clauses hold. Both spellings work here too — the expanded
`[[criterion.any_of]]` sections above, and `any_of = [{ require = [...] }, ...]`
as inline tables.

Write `require` and `any_of` on the same criterion and they are anded:
`require` is the precondition every route shares, `any_of` is the routes.

**The answer is the union, never the sum.** Routes overlap — a hand can hold
the Lantern *and* the Trinket Mage — so adding the branches would count that
hand twice, and on likely routes the total goes over 100%. Every path through
the enumeration either satisfies some branch or satisfies none, and contributes
its probability exactly once either way, so what comes back is P(A or B or C)
with no arithmetic for anyone to get wrong. This is also why it stays exact and
needs no second pass: a disjunction is answered in the same walk as everything
else, and it adds no query beyond the union of the ones its branches name, so
the enumeration is the same width as the same clauses written as separate
criteria.

The example above asks whether those cards were **drawn**, not whether they
could be cast. Castability is the mana model
([#10](https://github.com/cramt/progress-engine/issues/10)), so every route
there overstates itself, and the file says so rather than the number implying
otherwise.

**Which zone, and what silence means.** "Did I find the card" is not a
well-formed question; "is the card in this zone by this turn" is. A clause that
says nothing means `hand`, which is what every criteria file written before
zones existed already meant, so no existing number moves:

```toml
[[criterion]]
name = "loam in the yard by turn 5"
at_least = 0.80
require = [
  { turn = 5, query = 'name:"Life from the Loam"', zone = "graveyard", min = 1 },
]
```

| `zone` | What it counts |
|---|---|
| `hand` | the default. Cards drawn by that turn — nothing is cast or discarded yet, so nothing has left |
| `graveyard` | cards an effect routed to the yard — see [Effects](#effects). Zero in a run whose effects route nothing, and the run says so |
| `library` | cards matching the query that are still in the deck: the deck's total minus what has been drawn or binned |
| `battlefield` | refused. It would have to know what you could cast, and that is the mana model |

`graveyard` is reachable exactly when some effect in the run routes a card
there. In a run where none does, every count in it is zero by construction —
which is a confident zero, the failure this whole tool is about — so the run
says so rather than letting `0.00%` pass for a measurement:

```
$ progress-engine test loam.txt loam.criteria.toml
note: nothing routes a card to the graveyard in this run, so every count in
      it is zero by construction rather than by measurement.
      Asked by: "loam in the yard by turn 5"
      Declare `to_graveyard` on an [[effect]] to route one there.
     loam in the yard by turn 5    0.00%
```

Declare a destination and the note goes away, because the number is now a
measurement.

The same fact is in the JSON, as `zones`, alongside the query breakdown it is
the sibling of. Zones are discovered from the file the way queries are, so a
file that never says `graveyard` is never told anything about one.

`graveyard` is **unordered**, and that is a stated limit rather than an
oversight. Dredge cares which card is on top of the yard and this cannot say;
"Loam in the graveyard by turn 5" does not, and that is the north star it was
built for.

Everything the format refuses, it refuses by name, because each of these
otherwise produces a percentage that looks exactly like a real one:

| Written | Why it is refused |
|---|---|
| a criterion with neither `require` nor `any_of` | it asks nothing, so it holds on every hand: a confident 100% |
| `any_of = []` | a disjunction of no routes holds on none of them: a confident 0% |
| an `any_of` branch with no `require` clauses | that branch holds on every hand, so the criterion reports 100% whatever the other branches say |
| a clause with neither `min` nor `max` | it names a query and asks nothing of it |
| `min = 5, max = 2` | no hand can satisfy it: a confident 0% |
| `at_least = 70` | a threshold is a share of hands, so 70% is `0.70` |
| `zone = "battlefield"` | it would have to know what you can cast, and nothing here does |
| `zone = "exile"`, or any other zone | a zone that fell through to a default would answer the wrong question |
| `atLeast`, or any other unknown key | a key quietly dropped is an assertion quietly deleted |
| a file with no `[[criterion]]` and no `[[expect]]` | it asks nothing |
| `on = "cast"` on an effect | firing on the holding of a card overstates the turn by however many copies you hold, and knowing you cast it needs the mana model |
| `on` anything else, or `look = 0` | a trigger nothing fires, and a look that examines nothing |

Every one of those messages names the file, the question, and what was wrong
with it.

### Effects

Nothing in any card's data says a surveil land surveils one. The amount has to
be declared by somebody, and the product requirement is that the somebody is not
you — so a small library of effects ships with the tool and **autoloads on every
run**. There is no opt-in and no path to configure.

It is keyed on queries rather than on card names, which is the only reason it is
maintainable: keyed by card it would be thousands of entries and stale on every
set release, and keyed by query a new printing that surveils is covered the day
it exists. It is also not special machinery — these are `[[effect]]` tables in
exactly the syntax you would write yourself:

```toml
[[effect]]
match = "t:land otag:surveil"
look = 1
on = "landdrop"
to_graveyard = 'name:"Life from the Loam"'
```

| Key | What it means |
|---|---|
| `match` | which cards this is about, in Scryfall syntax |
| `look` | how many cards off the top it examines |
| `on` | when it fires. `landdrop` is the only one, see below |
| `to_graveyard` | the routing policy: which examined cards go to the yard. `"*"` is all of them, which is mill. Absent means none of them |

**Land drops only, on purpose.** Playing a land is free and hard-capped at one a
turn, so by turn *T* at most *T* of these have happened whatever your deck —
which is what keeps the enumeration bounded and exact. The mana-gated tier (Opt,
Preordain, tutors) is refused by name, because an opening hand of one Island and
six Opt casts *one* Opt, and a model that fires whenever you hold the card
overstates that turn sixfold. That needs
[#10](https://github.com/cramt/progress-engine/issues/10).

**The library never says where cards go.** Every shipped entry declares `match`,
`look` and `on`, and no entry declares `to_graveyard`. That split is the whole
design. The same Undercity Sewers wants Life from the Loam in the graveyard in
one deck and on top of the library in another — so the destination is part of
*your question*, not a property of the card, and the tool guessing at it would
be answering something nobody asked. An effect with no destination leaves every
card it looks at exactly where it was, which is where the next draw was going to
find it anyway, so it moves no number at all.

Write `to_graveyard` yourself and the same surveil starts binning:

```
$ progress-engine test loam.txt loam.criteria.toml
note: effect "t:land otag:surveil" (look 1, on landdrop, name:"Life from the Loam" to the graveyard)
      applies to 4 cards: Undercity Sewers
     loam in the yard by turn 5    4.92%
     loam in hand by turn 5       53.98%
```

**Overlap resolves last-wins, per card.** Query-keyed effects overlap by design:
`t:land otag:surveil` and `name:"Undercity Sewers"` both match the same card. So
every effect matching a card is collected and the **last declared** is applied —
never stacked, because a card matching two entries that each look 1 must look 1,
and looking 2 would be a confidently wrong number of exactly the shape this tool
exists to prevent. The standard library loads first, so your own entry overrides
it and no override syntax has to exist.

Because the library autoloads, it owes you an account of itself. A run says which
effect applied to which card, in the `effects` block of the JSON and in front of
a human, and the library's hash sits in `provenance` beside the deck and criteria
hashes — otherwise a number that moved because the shipped library changed would
be indistinguishable from one that moved because your deck did.

What ships today, and why each entry is there:

| `match` | `look` | Why |
|---|---|---|
| `t:land otag:surveil` | 1 | the only land drop whose other destination this engine models. Surveil bins to the graveyard, and the graveyard is a zone a criterion can ask about |
| `t:land otag:scry` | 1 | the same free, capped look at one card. Its other destination is the bottom of the library, which this engine cannot tell from the top, so it has no honest routing and never will until it does |

**What it costs.** Turning a card over mid-turn means the order of cards within a
turn starts to matter — drawing a surveil land and then surveilling is not the
same turn as surveilling and then drawing it — so a turn stops being one
checkpoint and becomes several. Each extra checkpoint multiplies the enumeration
by roughly the number of distinct groups, so a question that enumerates
comfortably without a routing effect can go over the ceiling with one, and come
back as an [estimate](#when-the-question-is-too-wide) rather than an exact
answer. That is the price of exactness rather than a bug: the alternative is
sampling the surveil inside an exact engine, which is a percentage nobody can
attribute. Ask about an earlier turn or name fewer queries to get the enumerated
answer back.

### How many, not just how often

A criterion answers *how often*, and for a long time that was the only question
this tool could be asked. But half of what anyone actually wants to know about a
decklist is *how many* — expected lands in an opening hand, ramp pieces by turn
three, mana available on turn four — and the only way to get at it was to write
five criteria with five different thresholds and difference them by hand:

```toml
[[criterion]]
name = "0 lands"
require = [{ turn = 0, query = "t:land", min = 0, max = 0 }]

[[criterion]]
name = "1 land"
require = [{ turn = 0, query = "t:land", min = 1, max = 1 }]

[[criterion]]
name = "2 lands"
require = [{ turn = 0, query = "t:land", min = 2, max = 2 }]
```

That is a histogram reconstructed by its reader, in their head, from a column of
percentages that do not say they belong together. It is arithmetic performed
somewhere nothing checks it, which is the same category of mistake as a
definition kept in your head rather than written down.

So there is a second kind of table. `[[expect]]` names one count rather than a
condition, and is answered with a mean and the whole distribution behind it:

```toml
[[expect]]
name = "lands in opener"
turn = 0
query = "t:land"
```

```
     lands in opener              mean 2.55
                                  0: 3.7%   1: 16.4%  2: 29.7%  3: 28.6%
                                  4: 15.7%  5: 4.9%   6: 0.8%   7: 0.1%
```

**The distribution is the more useful half, and it costs nothing.** The engine
already walks every composition with that composition's exact probability;
adding `p` to the bucket for the value a question took is the same loop over the
same terms as testing whether it held. So `P(exactly k)` comes out for every k,
exactly, rather than estimated — and it is what a mean cannot tell you. "2.55
lands on average" is equally true of this deck and of one that mulligans to
oblivion a fifth of the time; the row underneath, which says 3.7% of opening
hands have no land at all and 20.1% have at most one, is the thing anybody
brewing actually wants to look at. A summary statistic that hides its own shape
is a confident number about the wrong question, which is this repository's
subject.

`--simulate` answers both kinds. A sampler computes a mean by averaging and a
distribution by histogramming, so the second question costs it no more than the
first, and the two engines are held to each other bucket by bucket rather than
only on the mean — a mean can be right while the shape underneath it is wrong.

**A value is a whole number of cards, and nothing else can be written.** The
answer is a histogram, so every value it accepts needs a bucket of its own. An
expectation names one query at one turn, and the only thing that comes back from
that is a count, so a value with nowhere to go is not something the format can
express. Dividing by seven to report a rate is a reasonable thing to want, and
asking for it here used to earn a refusal naming the expectation, because the
JavaScript that could write `count('t:land') / 7` could also write it by
accident. The alternatives were rounding 0.57 to 1, or picking a bucket width
nobody asked for — both of which answer a different question than the one
written down and leave no mark in the output saying so. The refusal is still in
the engine, at the boundary where a count is built; there is simply no longer a
front end that can reach it.

The same reasoning keeps the two kinds apart. They used to be two registration
functions in a language that coerces a number to a bool and a bool to a number
without complaint, so a criterion that forgot its comparison silently became "at
least one land" under a name promising a count, and an expectation answering
`true` reported a probability in a column headed *mean*. Both had to be caught
at runtime and reported by name. They are now two different tables with two
different sets of keys: `[[criterion]]` takes `require` and `[[expect]]` does
not, so writing one and meaning the other is not a mistake the file can hold.

**A wide distribution is windowed, and says what it left out.** Lands seen by
turn twenty runs from zero to twenty-six, and printing twenty-seven buckets is
not a histogram but a wall. The human output shows the twelve contiguous buckets
holding the most mass — contiguous, because a histogram with holes punched in it
reads as missing data — drops ends that would print as `0.0%`, and states the
remainder rather than dropping it:

```
$ progress-engine test simple-ramp.txt lands-by-turn.criteria.toml
     lands by turn 20  mean 9.45
                       4: 0.6%    5: 2.0%    6: 5.1%    7: 9.9%    8: 15.1%
                       9: 18.4%   10: 18.0%  11: 14.2%  12: 9.0%   13: 4.7%
                       14: 2.0%   15: 0.7%  (+0.4% outside)
```

The JSON carries the whole array untruncated, indexed by value, so nothing is
actually lost — the summary can afford to be a summary precisely because the
machine-readable half is complete. A summary that quietly mislaid four percent
of its mass would be the failure this whole document is about, in miniature.

**There is no assertion on an expectation, and that is a stated gap rather than
an oversight.** `at_least` on a criterion is a threshold on a probability: a
number between zero and one that means the same thing in every criterion ever
written. The same keyword here would be a threshold in the units of whatever is
being counted — "at least 2.5" is lands in one line and mana in the next — and
nothing in the report would say which reading applied. One word with two
meanings is precisely the unstated definition this tool exists to eliminate, so
rather than ship the trap, an expectation is informational and cannot fail a
run. It does not appear in `asserted`, it cannot move `failed`, and it never
touches the exit code.

The assertion people actually want is probably not about the mean anyway.
"Averages at least 2.5 lands" is satisfied by a deck that floods half the time
and is screwed the other half, which is the same shape-hiding this feature
exists to undo. "Has two lands 90% of the time" is a statement about a
percentile, and a percentile is a statement about a range, which is
[issue #12](https://github.com/cramt/progress-engine/issues/12). Naming that
assertion is left until the thing it asserts on exists, and it will not be
called `at_least`.

### The card index it builds

`parse` reads the decklist text and nothing else, so it needs no card data and
never has. `test` has to know what a card *is*, and `sync` is where that comes
from:

```
$ progress-engine sync
downloading oracle_cards, updated 2026-09-04T09:01:54.392+00:00 (24.5 MB)
read 38631 records
  skipped 3321: not a card (token, emblem or art card)
  skipped 8: not English
  kept 35224 cards
  40 names match more than one card; which printing is kept is decided by a rule that gives the same answer every sync
wrote 35224 cards to /home/you/.cache/scryfall/index.jsonl
```

It writes `index.jsonl` under `$SCRYFALL_CACHE`, failing that
`$XDG_CACHE_HOME/scryfall`, failing that `~/.cache/scryfall`; `--index <path>`
puts it somewhere else, and `--from <file>` builds from a bulk file already on
disk rather than downloading one. A run with no index at all says so and names
the command that fixes it.

**A run costs what your deck costs, not what Magic costs.** The index is a
header line and then one card per line, each behind the key it is filed under:

```
$ head -c 130 ~/.cache/scryfall/index.jsonl
{"schema":1,"updated_at":"2026-09-04T09:01:54.392+00:00","cards":35224,"keywords":["... Catch","10,000 Needles", ...
$ grep -c . ~/.cache/scryfall/index.jsonl
35225
```

The header holds what is true of the file rather than of any card, so a question
about the file answers off one line. The keyword list is there because `kw:`
needs the whole pool to know that `kw:flyign` is a typo rather than a card
nobody plays, and it is the pool: Scryfall files *... Catch* as a keyword, so
this does too.

Opening it locates the lines and parses none of them; a card is parsed when
something names it. A decklist names about a hundred, so a `test` run reads a
hundred cards rather than thirty-five thousand — 2.94s to 0.22s on this machine,
measured over ten runs of the same deck against the same 35,220-card index:

| | mean |
|---|---|
| one JSON object, parsed whole | 2.937s ± 0.131 |
| one line per card, parsed on demand | **0.222s** ± 0.006 |

The cost was never the 24MB — it is facet-json's per-field reflection over
35,224 records of twenty-odd fields each, which is why the file is the same size
either way. Of the 0.222s that was left, about 0.12s was proportional to the
index — reading it and finding its lines — and about 0.10s was starting a
JavaScript runtime and answering the question. Criteria are TOML now, so there
is no runtime to start, and the same ten runs of the same deck against the same
index come in at **0.136s** ± 0.008. `sync` gets the same deal on the other half
— deciding whether the index it already has is Scryfall's latest reads one line
instead of the whole file.

Two things follow from writing the key beside the card rather than deriving it.
The file stays greppable, so `grep -P '^sol ring\t'` is a working lookup with no
tool at all. And the two can disagree — so a card read out from under somebody
else's key is an error rather than the wrong card returned confidently.

Splitting a document into lines also adds a failure it did not have: JSON cut in
half stops parsing, whereas a list of lines cut in half reads as a shorter list,
which here would be a card pool quietly missing a thousand cards. So the header
says how many lines should follow, and an index that does not have them is
refused by name.

Loading the library is strict about cards the index does not know: an unknown
card stops the run rather than being skipped, because a card that cannot be
looked up has no type line and would quietly skew every probability it touches
— the same reasoning that makes an excluded card announce itself.

**Every record is accounted for by name, not as a total.** "Kept 35,224 cards"
is not a statement anybody can check; "dropped 3,321 as tokens" is, because it
moves when Scryfall's data moves. The skip reasons are the whole audit trail for
a file nothing else in this repository can see inside.

**A sync can refuse to install what it built.** It overwrites the file every
later run reads, so the failure mode is not "sync did nothing" but "every number
from now on describes a card pool that does not exist". Two things stop it: more
than a handful of records whose shape this tool does not expect, which means
Scryfall's format has moved under the flattening below; and a download that
yields a fraction of the card pool, which means it was truncated. A truncated
download still parses as perfectly valid JSONL, which is exactly why the count
is checked rather than trusted. The write itself goes to a neighbouring file and
is renamed over the target, so an interrupted sync leaves the previous index
intact instead of a half-written one.

**Tokens are not cards, and used to win.** Scryfall prints tokens that share a
name with a real card. Keyed by lowercased name they collided, and whichever was
written last won the key — so **Llanowar Elves was a mana value 0 token that is
not legal in Commander**, along with forty other cards including Mutavault and
Meteorite. Every field belonged to the token, so `mv>=5` silently missed a
Meteorite and colour identity came back empty for all of them. `t:land` happened
to survive because `Token Land` still contains the word, which was luck rather
than design.

The discriminator is the `layout` field and nothing else. `set_type` cannot do
it — tokens ship in sets typed `memorabilia`, `promo`, `masters` and `box`, and
the `emblem` layout ships in sets typed `token`. Nor can the word "Token" in the
type line, which fifty token records do not carry. Owning the build is what made
the fix expressible at all: once two objects share a key the information needed
to tell them apart is gone, and the index was built elsewhere.

**What the index carries, and what that costs.** Beyond the type line and mana
value it always had: the mana a card actually *produces*, its colours as
distinct from its colour identity, its printed mana cost, power, toughness,
loyalty and defense per face, rarity, set, layout, and every format's legality
word. Loading it takes about 2.9 seconds against the 1.8 the old five-field
index took. That is a real cost for real data, and it is stated here rather than
left to be noticed: the twenty-three legalities are stored as one letter each
because the readable form was 17MB of a 50MB file and three seconds of every
run, and default-valued fields are not written at all, which is why an index
carrying four times as much is slightly smaller than the one it replaces.

**Faces are flattened, and the invariant is checked rather than assumed.**
Scryfall puts oracle text either on the card or on its faces, never both and
never neither — so every card in the index comes out with at least one face,
including the single-faced ones, and nothing downstream has to branch on how
many there are. Without that, `power` means the card's power on one layout and
nothing at all on another, and `pow>=3` would answer about the front of Delver
of Secrets rather than about the card. A record that violates the invariant is
counted and named rather than quietly flattened wrongly.

**Reminder text is separated at sync time**, because Scryfall's `o:` does not
search it and its `fo:` does. Without the split, `o:flying` is satisfied by
"(This creature can't be blocked except by creatures with flying)" — the
miscategorisation this document opens with, wearing a different card. Where a
card's brackets do not balance, which a handful of split reminder cards manage,
the text is left whole rather than truncated at the stray bracket: `o:` matching
some reminder text is a far smaller error than `o:` losing a card's actual rules.

Oracle tags — `otag:ramp`, `otag:sacrifice-outlet` — are now published as bulk
data too, which removes the objection that blocked
[issue #15](https://github.com/cramt/progress-engine/issues/15). They are not
synced yet.

### The query language

A subset of [Scryfall's search syntax](https://scryfall.com/docs/syntax), plus
one addition of our own. Where a key exists it means what Scryfall means, and
where one does not, using it is an error naming the key rather than a query that
matches nothing.

| Key | Asks |
|---|---|
| `t:` `type:` | Substring of the type line |
| `o:` `oracle:` | Oracle text, **without** reminder text |
| `fo:` `fulloracle:` | Oracle text, reminder text included |
| `name:` | Substring of the name; a bare word means this |
| `kw:` `keyword:` | One whole keyword ability the card has |
| `mv:` `cmc:` | Mana value, including `mv:even` and `mv:odd` |
| `c:` `color:` | The card's own colours |
| `id:` `identity:` | Its colour identity |
| `produces:` `prod:` | The mana it can actually make |
| `pow:` `tou:` `pt:` `loy:` `def:` | Printed statistics, comparable to each other |
| `r:` `rarity:` | Rarity, ordered so `r>=rare` works |
| `s:` `e:` `set:` | Set code of the printing the index carries |
| `f:` `banned:` `restricted:` | Format legality |
| `m:` `mana:` | The printed cost, as a multiset of symbols |
| `devotion:` | How much a permanent gives to a devotion count |
| `layout:` | Scryfall's layout name |
| `is:` `not:` | See below |
| `otag:` `oracletag:` | A Scryfall oracle tag the card is in |
| `cat:` `category:` | **Ours**: an Archidekt category from the decklist |

All of them combine with juxtaposition for AND, `or`, `-` for NOT, and
parentheses.

**`c:` and `id:` point in opposite directions**, which is the single most
misread thing in Scryfall's syntax and the reason both are spelled out here.
`c:rg` means red **and** green — a colon is "contains all of". `id:rg` means
fits inside Gruul — a colon is "is contained by". Paste `c:wu` when you meant
`id:wu` and you get a confident answer about a different deck. Both take colour
letters, full names, guild, shard, wedge and college nicknames, `c` for
colourless, `m` for multicolour, and a bare number to count colours.

**`produces:` is the fix for the bug this document opens with.** Kor Haven's
`{W}` is in an activation cost, so `o:"{W}"` calls it a white source and
`produces:w` does not. It is orthogonal to `t:land`, so `t:land produces:w` and
`-t:land produces:w` are both askable, and multiple letters mean AND as they do
for `c:`.

**A statistic that is not a number satisfies no comparison.** `*`, `1+*`, `∞`
and `.5` are all real printed power values. Calling `*` zero would put Tarmogoyf
in `pow=0` and quietly out of `pow>=1`; instead neither holds, and `-pow>=1` is
where "we cannot say" lands. Statistics are read on **every face**, so `pow>=3`
finds Delver of Secrets, which is a 1/1 that becomes a 3/2.

`is:` answers `permanent`, `spell`, `historic`, `vanilla`, `bear`, `dfc`,
`mdfc`, `transform`, `split`, `flip`, `meld`, `leveler`, `adventure`, `hybrid`,
`phyrexian`, `commander`, `partner`, `companion`, `reserved` and `gamechanger`.

**Every one of them was checked against Scryfall's own answer**, by counting
the whole card pool locally and asking Scryfall for the same count. Four came
back identical and the rest within the handful of cards our index holds and
Scryfall's default search hides. Three were wrong and are not any more:
`is:phyrexian` read only the mana cost, and most Phyrexian mana is in an
activation cost — Blinding Souleater costs `{3}` — which missed thirty-three of
seventy-three cards. `is:partner` matched only the word "partner", when the
mechanic also prints as "Choose a background", "Doctor's companion" and
"Friends forever", and a Background carries no keyword at all because it is a
subtype; that missed eighty-five. `is:hybrid`, unlike `is:phyrexian`, is read
from the printed cost alone, which is Scryfall's asymmetry rather than ours and
was found the same way.

Writing a derivation and reading it back is not the same as checking it. All
three of those looked right.

**`otag:` is how the curated lists get here.** Scryfall's oracle tags — what a
card *does*, as opposed to what it says — are not in the bulk data. They are not
derivable from it either: a naive `/enters.*tapped/` calls a Temple and a
Shockland the same thing, and "enters tapped unless you control two or fewer
other lands" is conditional in a way no regex survives.

So they are not derived. They are **fetched**, from Scryfall's search API at
`sync` time, and the index records the date it asked. That is the same standard
the rest of this file is held to: not a hard-coded copy that goes stale, and not
a guess dressed as a fact, but somebody else's answer with a date attached.

`sync` fetches six tags today, each because something here reads it:

| Tag | Cards | Read by |
|---|---|---|
| `tapland` | 495 | whether two lands are actually two mana |
| `surveil` | 334 | how many cards deep a turn sees |
| `scry` | 475 | the same, leaving the card on top |
| `mill` | 1,305 | the graveyard as a destination |
| `tutor` | 1,168 | selection over the whole library |
| `ramp` | 2,316 | the mana-curve questions |

They cost nothing to carry: 5,197 of 35,486 cards are tagged, the file is the
same 24MB, and a run parses only the cards your deck names either way.

**An index carries the tags it was told to fetch, and says which.** The header
lists them, so `otag:` can tell *this index never asked about that tag* apart
from *no card is in it* — the same empty result, and very different facts. A
query naming a tag the index does not carry is an error naming the tag, for the
same reason `kw:tramp` is.

Note that `is:fetchland` (the ten-card cycle) and `otag:fetchland` (54 cards
that fetch) are different questions, and Scryfall means both.

**`is:frenchvanilla` was implemented and then removed**, which is the same rule
applied to our own work. Scryfall's `keywords` array mixes ability words —
"Mark of Chaos Ascendant" — in with real keyword abilities, and both are
followed by an em dash and then prose, so nothing in the data tells them apart.
Scryfall's own answer also excludes keywords with numeric parameters, `Modular
3` and `Rampage 3`, for reasons no field records. Every derivation tried landed
twenty per cent away from Scryfall in one direction or the other. A key that
looks like Scryfall's and quietly disagrees with it is worse than one that says
it is missing, so it says it is missing.

**`f:` selects cards, and that is all it does.** `f:modern` asks whether a card
is legal in Modern, which is a fact about that card and a perfectly good way to
pick one; so are `banned:legacy` and `-f:commander`. What used to sit beside
this was a check on the *list* — five Commander rules and a `WARNING: this is
not a legal Commander deck` block above your numbers — and it is gone
([#42](https://github.com/cramt/progress-engine/issues/42)). The math does not
need a format: hypergeometric probabilities care about library size and
composition, both of which come from the decklist rather than from any rule set,
and having opinions about Commander specifically made a Modern list
second-class for no mathematical reason. Selecting is what a query does;
volunteering a verdict about your deck is not what this tool is.

**`m:` and `devotion:` treat a cost as the multiset it is.** `{2}{W}{W}` is two
generic and two white, so `m:{W}{W}` contains it and `m>{2}{W}{W}` does not.
A colon is "contains at least", as on Scryfall, and `=` is exact. Shorthand
works for symbols that are not split — `m:2WW` — and a hybrid reads the same
written either way, because `{U/W}` and `{W/U}` are one symbol; a Phyrexian
`{W/P}` is not reordered, since the marker's position is fixed and sorting it
would invent a symbol.

Each **face** is costed separately. Wear // Tear is stored as `{1}{R} // {W}`,
and reading that as one multiset would invent a three-mana two-colour spell
nobody can cast — so `m:{W}` and `m:{1}{R}` both find it and `m:{R}{W}` does
not. Devotion is counted only on **permanents**, because only a permanent is on
the battlefield to give it, and a term naming two different colours is refused:
`devotion:{u}{b}` is two questions, while `devotion:{u/b}{u/b}` is one.

### Queries that match nothing

The defining failure mode this tool exists to prevent is a query that is
perfectly valid and matches no cards. It cannot be a parse error, and it yields
a confident 0% rather than a complaint. So every run reports what each query
matched, and says so loudly when that is zero:

```
$ progress-engine test simple-ramp.txt typo.criteria.toml
note: query "cat:\"Rmap\"" matched no cards in this deck
FAIL misspelled category   0.00%  (needs 30.0%)
```

Unsupported *syntax*, by contrast, is refused outright — `otag:ramp` names
itself as an error rather than quietly matching nothing, and where the accepted
values are a closed set the message lists them:

```
-engine test simple-ramp.txt typo.criteria.toml
Error: in query "f:pauperr": "f:pauperr": "pauperr" is not a format (supported:
standard, future, historic, timeless, gladiator, pioneer, modern, legacy, pauper,
vintage, penny, commander, oathbreaker, standardbrawl, brawl, competitivebrawl,
alchemy, paupercommander, duel, oldschool, premodern, predh, tlr)
```

That is why the format table is a closed struct rather than a map: a map would
make every misspelling a silent 0%.

### Cards that are never in your library

Sticker sheets, attractions, planes, phenomena, schemes, vanguards, conspiracies,
dungeons and emblems live in a deck of their own or in no deck at all. Counting
them inflates the library and moves every probability with it: a 99-card list
with ten attractions answers questions about a 109-card library that does not
exist. They are left out of the library, and never quietly:

```
$ progress-engine test unfinity.txt criteria.toml
note: 11 cards never in the library and not counted: 1x Ancestral Hot Dog Minotaur (Stickers), 3x Bumper Cars (Attraction), ...
```

The same list appears in the JSON as `excluded`, with the card type that did it.
Dropping cards silently would be the empty-query failure again — a confident
number nobody can question — so someone whose 109 comes back as 99 is told what
left and why.

The decision is made on the **type line**. Category names cannot do it: "Sticker
Package" is a legitimate Archidekt category for the real cards that *apply*
stickers, and matching it once dropped five of them from a 100-card list. Nor is
it a substring match, because `Plane` sits inside every planeswalker ever
printed. The type line is split on its em dash and compared whole word by whole
word, card types on the left and Attraction on the right, where it is a subtype
of `Artifact — Attraction`.

### Questions the deck cannot answer

Two more routes to a confident number about nothing, both refused rather than
answered:

```
$ progress-engine test all-commander.txt criteria.toml
Error: the library is empty: every card in the list is a commander or outside the deck

$ progress-engine test two-card-deck.txt criteria.toml
Error: this question draws 7 cards from a library of 2
```

The second is refused identically under `--simulate`. Left to themselves the two
engines disagree by the whole answer: enumeration finds no dealable hand and
reports 0%, while sampling deals what it can and reports whatever that gives.

Neither falls back to sampling the way a
[too-wide question](#when-the-question-is-too-wide) does, and the difference is
the point: a question that was only expensive still has an answer worth
estimating, and one that was never modelled does not.

## Decklist format

Archidekt's export format. Quantity and name are the only required parts:

```
1x Sol Ring
3x Plains
1x Lightning Bolt (sos) 267 *F* [Interaction]
1x Myr Battlesphere (tdc) [Big Colorless,Test]
1x Rashmi and Ragavan [Commander{top}]
1x Lurrus of the Dream-Den [Companion{noDeck}]
// line-leading double slashes are comments
```

Notes:

- **Multiple categories** are comma-separated inside the brackets.
- **`//` is a comment only at the start of a line.** Card names contain it —
  `Unstable Glyphbridge // Sandswirl Wanderglyph` — and treating it as an inline marker would
  truncate every double-faced card.
- **Malformed lines are errors, not silences.** A line that cannot be parsed names itself and
  its line number. The predecessor to this parser dropped them quietly, so a typo surfaced much
  later as a mysteriously wrong card total.
- `Commander` and `Companion`/`Sideboard`/`Maybeboard`/`{noDeck}` are matched **per category**,
  so `[Ramp,Commander{top}]` is still your commander.
- **`outside` is a fact about the decklist, not about the cards.** It is what the
  list says — companions, sideboards, `{noDeck}` — and that is all `parse` can
  know, having no card index. A sticker sheet is outside the library too, but
  nothing in the text says so, so it leaves later, at `test` time, where card data
  is at hand, and is reported separately as `excluded`.

## Development

```bash
nix develop        # devshell with the toolchain
cargo test
nix flake check    # fmt, clippy -D warnings, tests, build
```

Reflection comes from [facet](https://github.com/facet-rs/facet): `facet-json`
writes the JSON contract above, reads Scryfall's bulk data and reads and writes
the cards in the index, `facet-toml` reads criteria files, and `figue` parses
argv. One `#[derive(Facet)]` per type feeds all of them.

Criteria schema types carry `#[facet(deny_unknown_fields)]`, and that is
load-bearing rather than tidy. Without it `atLeast` is silently dropped and a
criterion that was supposed to assert something reports an informational number
instead — a passing run for a test nobody is running any more.

The tests never reach the network. `sync --from <file>` builds from a bulk file
on disk, and the fixtures are real Scryfall records checked in verbatim — a test
whose answer depends on Scryfall being up is a fact about reachability rather
than about this code.

figue treats a missing argument as a help request, printing usage to stdout and
exiting 0. Both halves of that are wrong here — stdout carries JSON a caller
pipes through `jq`, and other tools read the exit code — so usage errors are
sent to stderr with a non-zero status instead, and stdout stays JSON-only.

## License

MIT

## Crate layout

A workspace, split so each piece can be understood — and depended on — without
dragging in the others.

| Crate | Responsibility | Knows about |
|---|---|---|
| `pe-stats` | Exact hypergeometric draw probabilities | Nothing. No Magic concepts at all. |
| `pe-decklist` | Parsing Archidekt decklists | Decklist text. No card data. |
| `pe-scryfall` | Card data, Scryfall bulk data and search syntax | Cards. No decklists. |
| `pe-criteria` | Grouping cards by query, applying effects, evaluating exactly | Counts, the zones they are counted in, and where a looked-at card goes. Not cards, and not where the questions came from. |
| `pe-toml` | Reading a criteria file and answering it, and shipping the standard effect library | The criteria format, and counts. No cards. |
| `pe-sim` | Sampling, validated against `pe-stats` | Shuffling. |
| `pe-cli` | The `progress-engine` binary | All of the above. |

The seam worth knowing about is between `pe-scryfall` and `pe-decklist`: a query
can filter on `cat:"Exile Outlet"`, which is decklist data, not card data. Rather
than have the card crate depend on the decklist crate, `CardView` takes
categories as a plain `&[String]`. The CLI is what joins the two, which keeps
both halves independently testable.

The same seam decides where each kind of "outside the library" lives. Companions
and sideboards are decklist data, so `pe-decklist` answers those; sticker sheets
and attractions are card data, so `pe-scryfall` answers those. Neither crate
learns about the other, and `pe-cli` applies both at the point where a decklist
entry finally meets its card.

Legality is on the card side of that seam and stays entirely there. Whether a
card is banned, whether it may be repeated at all, whether its type line puts it
in the command zone and whether its identity fits inside a given one are all
facts one card settles alone, so they live in `pe-scryfall::legality`, which is
what `f:`, `banned:`, `restricted:`, `is:commander` and `is:partner` read. The
half that needed the decklist — copy counts, which lines were nominated,
how many cards there are altogether — used to live in `pe-cli` and has been
removed ([#42](https://github.com/cramt/progress-engine/issues/42)): selecting
cards by what a format says about them is a query, and pronouncing on a whole
list is not something a draw-probability engine has any business doing.

`pe-stats` deliberately has no idea what a card is. Its tests are pure
known-answer arithmetic, so a failure there is unambiguously a maths bug rather
than a card-data bug. The same reasoning puts the evaluator behind a trait in
`pe-criteria`: the enumeration is tested with plain Rust closures, so a failure
there is an engine bug and a failure in `pe-toml` is a criteria-format bug.
Keeping those distinguishable is worth the indirection.

`pe-toml` holds the only `impl Evaluator`, and both engines take it through the
same trait. That is why swapping the criteria format out from under them was a
new crate and a deleted one rather than a change to either engine — and why
there is one evaluator serving both rather than two that can disagree.

`pe-sim` exists to check `pe-stats`, not to replace it. Where both can answer,
they must agree — and that agreement is asserted at three levels: unit, through
the criteria layer both engines share, and end to end through the binary.
