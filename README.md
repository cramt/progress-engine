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
chase — and it is fast enough that the answer is instant. The enumeration is sized **per
question rather than per file**, so an easy question costs what it costs and not what the
hardest question beside it costs — see [one enumeration per question](#one-enumeration-per-question-not-per-file).
A question too wide to enumerate even on its own is
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

### One enumeration per question, not per file

Compositions multiply with the number of groups the queries split the library into and with
how many checkpoints the walk carries. Both used to be sized **once, for the whole file**, as
the join of what every question in it needed — so a `can_cast` clause, which has to tell a
Plains from an Island, split a real Commander manabase into seventeen groups and then charged
all seventeen to the criterion beside it that only ever asked `count('cat:"Ramp"')`.

Since [#31](https://github.com/cramt/progress-engine/issues/31) the questions are partitioned
into classes and each class gets the cheapest enumeration that can answer it. Two things
shrink:

- **Groups.** A class keeps only the queries it reads, plus whatever the walk itself reads —
  which cards a live effect applies to, where it routes them, the land-drop priority — and
  keeps what a land makes only if something asks whether a cost could be paid. Everything else
  merges.
- **Checkpoints.** A class keeps only the turns it names. "Loam in hand by turn 5" is one
  multivariate hypergeometric over eleven cards, not a path through five checkpoints. A
  criterion that correlates two turns names both and keeps both, because the hands that get
  there late are exactly the ones it excludes; a question about what is on the battlefield or
  about paying a cost keeps every turn up to its own, because one land drop a turn is
  use-it-or-lose-it and no total can say that.

Measured on the two decks in `decks/`, against an index synced with oracle tags:

| File | Before: groups / compositions | After: widest class | Before | After |
|---|---|---|---|---|
| `lantern.criteria.toml` | 7 / 4,120,116 | 6 / 72,072 | exact, 2.6s | exact, 0.20s |
| `lantern.criteria.toml --draw` | 7 / 28,840,812 | 6 / 108,108 | **sampled** | **exact**, 0.20s |
| `loam.criteria.toml` | 4 / 30,720 | 3 / 55 | exact, 0.19s | exact, 0.16s |
| the same file plus `can_cast = "{1}{U}"` at turn 4 | 17 / 1,204,456,341 | 17 / 1,204,456,341 for that one clause; ≤ 72,072 for the other six questions | **all six sampled** | **one sampled, six exact** |

Every number that was exact before is the same number after, to every digit the report
prints. Narrowing is not an approximation: a coarser grouping is a marginal of the finer one,
and the draws between two turns nobody reads have the same joint distribution merged as
separate. It is asserted as a property over generated questions and end to end through the
binary.

The cost that remains is real and is not hidden: a `can_cast` clause on a Commander manabase
is a billion compositions at turn four whatever else is in the file, and it is still
[sampled](#when-the-question-is-too-wide). What changed is that it no longer takes its
neighbours with it.

### When the question is too wide

A question that is still over the ceiling on its own — a criterion correlating two turns
across seven queries, say — used to be refused. It is now **answered by sampling, loudly**:

```
$ progress-engine test simple-ramp.txt too-wide.criteria.toml
ESTIMATE: a question here was too wide to enumerate exactly: 103169430
          compositions across 12 groups, against a ceiling of 5000000. It was
          answered by sampling 200000 hands instead.
          1 of the 3 questions here needed that, and it is the one
          quoted with a ±:
          everything at once by turn 6, off a turn-2 land
          Everything else below was enumerated exactly. A difference
          smaller than a figure's ± is not a difference.
          Pass --exact to refuse a question this wide rather than estimate it.
PASS everything at once by turn 6, off a turn-2 land   39.16% ± 0.11  (needs 10.0%)
     a land by turn 6                                  99.71%
```

The loudness is the whole safety argument, and it is why this is acceptable at all. A silent
fallback would be a number quietly changing kind — an estimate wearing an exact answer's
clothes — which is the failure this repository is named against. So the warning is the first
thing on stderr, it names the questions it applies to, every sampled figure is quoted with its
error bar wherever it appears and every enumerated one is quoted without, and each answer in
the JSON carries its own `"method"`. The top-level `"method"` is `"exact"`, `"sampled"` or
`"mixed"`, with `"sampled_because": "too_wide"` and the width of the widest class it refused.
A labelled estimate is a different answer to a question that was honestly too big
([#48](https://github.com/cramt/progress-engine/issues/48)).

The refusal is still there under `--exact`, for anyone who would rather have no answer than an
approximate one — CI, and the cross-engine agreement tests, which need an oracle that either
enumerates or says nothing:

```
$ progress-engine test simple-ramp.txt too-wide.criteria.toml --exact
Error: this question is too wide to answer exactly: 103169430 compositions across 12 groups.
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
`min` and `max` — or a `turn` and a `can_cast`, for which see
[Mana, as a gate](#mana-as-a-gate). A criterion can also hold `any_of`, a list
of alternative routes, for which see [Routes](#routes-any_of). An `[[expect]]`
is a `name`, a `turn`, a `query` and an optional `zone`, and reports a
distribution rather than a verdict. A file may also hold `[[effect]]` tables,
for which see [Effects](#effects), and one `[land_drop]` table saying which
land you would play when you could play either, for which see
[Mana, as a gate](#mana-as-a-gate).

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
could be cast. Whether you could pay for them is
[Mana, as a gate](#mana-as-a-gate) below.

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
| `battlefield` | **lands you have played**, one drop a turn — the ones your `[land_drop]` priority played, or the most you could have played if you declared none. Answerable only for a query that matches lands; refused for anything that would have to be cast |

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
| `zone = "battlefield"` on a query matching a spell | a land arrives on a land drop and this engine walks those; a spell has to be cast, and which spells you cast when you cannot cast them all is the budget half of [#10](https://github.com/cramt/progress-engine/issues/10) |
| `zone = "exile"`, or any other zone | a zone that fell through to a default would answer the wrong question |
| `query` and `can_cast` in one clause | two different questions, one of which would have to be answered silently |
| `can_cast = "{X}{G}"`, or any hybrid or Phyrexian symbol | each is a decision about how much to pay rather than an amount; read as zero, an X-spell is castable on turn one |
| a mana question in a file with a live `to_graveyard` effect and no `[land_drop]` | both decide which land you played this turn, and they decide it differently. Declare the priority and they are one decision |
| `[land_drop]` with no `prefer` entries | it settles no drop, and the run would report a policy that decided nothing |
| a `prefer` entry repeating an earlier one | the earlier one already took every land it names, so it can never decide a drop |
| `atLeast`, or any other unknown key | a key quietly dropped is an assertion quietly deleted |
| a file with no `[[criterion]]` and no `[[expect]]` | it asks nothing |
| `on = "cast"` on an effect | firing on the holding of a card overstates the turn by however many copies you hold, and knowing you cast it needs the mana model |
| `on` anything else, or `look = 0` | a trigger nothing fires, and a look that examines nothing |

Every one of those messages names the file, the question, and what was wrong
with it.

### Mana, as a gate

Two lands is not two mana. A land that enters tapped makes none the turn it
arrives, a land you drew on turn four is not a land you played on turn two, and
one Hallowed Fountain is a white source and a blue source and one mana.

So there are two questions here, and this is the first one:
**could I have paid for it by turn N?** The other is mana as a *budget* — an
opening hand of one Island and six Opt casts one Opt, because the first one
spends the Island — and that needs sequencing and is not built. See
[#10](https://github.com/cramt/progress-engine/issues/10).

**Lands you have played.** `zone = "battlefield"` counts them, and a land drop
is one a turn and use-it-or-lose-it:

```toml
[[criterion]]
name = "three lands in play by turn 3"
require = [{ turn = 3, query = "t:land", zone = "battlefield", min = 3 }]
```

Five lands drawn by turn 3 is three lands in play, not five, because the other
two drops never happened. That gap is what every mana-relevant criterion in this
repository quietly assumed away before this shipped.

**Whether a cost is payable.** `can_cast` takes a mana cost as printed:

```toml
[[criterion]]
name = "the commander on turn 3"
at_least = 0.60
require = [{ turn = 3, can_cast = "{1}{G}{G}" }]
```

This is a primitive rather than something you assemble from counts, and the
reason is worth a paragraph. Written by hand it would be `produces:w` at
`min = 1` and `produces:u` at `min = 1`, and both of those are satisfied by one
Hallowed Fountain, which cannot pay `{W}{U}`. Whether a set of lands covers a
set of pips is a **matching** — can these sources be assigned to these symbols —
and no arithmetic over independent counts answers it. The engine solves it
exactly, per composition, because a composition knows the lands jointly.

Generic takes any land, `{C}` takes a land that makes `{C}`, and `{X}`, hybrid
and Phyrexian symbols are refused by name: each is a decision about how much to
pay rather than an amount, and reading `{X}` as zero makes an X-spell castable
on turn one.

**What it assumes about tapped lands, and how it says so.** Scryfall's
`otag:tapland` is 495 lands that always enter tapped. It does not include the
shocklands, which are `otag:conditional-tapland` — *"you may pay 2 life; if you
don't, it enters tapped"* is a decision, not a property. This tool takes the
pessimistic half of that decision, and does not take it quietly:

```
note: one land here lets the pilot decide whether to enter tapped. This run assumes they do:
      Hallowed Fountain.
      A shockland's 2 life is a decision no criteria file has made yet, so the pessimistic
      reading is taken: it makes no mana the turn it arrives. Every number below that
      depends on one of these is a floor rather than a measurement.
```

The same list is in the JSON as `assumed_tapped`. It understates a real
shockland manabase, which is the direction this tool prefers to be wrong in: a
number your deck can beat is a better failure than one it cannot reach. Making
the choice declarable per file is the obvious next step and is deliberately not
a default nobody stated.

**What it costs.** Counting lands in play is free — it reads the land drops the
enumeration already walks and adds no group and no path. `can_cast` is not free:
it has to tell a Plains from an Island, so lands split into one group per
*(what it produces, does it arrive tapped)*, and the enumeration widens with the
group count. Measured on the decks in `decks/`, against an index synced with
oracle tags, asking `can_cast = "{1}{U}"`:

| Deck | Land groups | Turn 4 |
|---|---|---|
| `lantern.txt` | 17 | 1,204,456,341 — over the ceiling, [sampled](#when-the-question-is-too-wide) |
| `loam.txt` | 18 | 2,018,478,528 — over the ceiling, sampled |

A real Commander manabase is a dozen-plus profiles, so this is the ordinary
case rather than a pathological one, and turn **four** is where it lands. A deck
whose manabase is three kinds of basic stays exact much deeper — five groups is
41,250 compositions at turn 4 and a million at turn 6. The matching itself is
cheap, about 0.65µs a hand; the width is what costs.

Since [#31](https://github.com/cramt/progress-engine/issues/31) that width is
charged to the clause that asked for it and to nothing else: the other six
questions in `lantern.criteria.toml` are enumerated exactly beside it, at 72,072
compositions or fewer. What is still true is that the castability clause itself
is an estimate past the opening turns on a real manabase, and
[#55](https://github.com/cramt/progress-engine/issues/55) is the next narrowing
that would change that.

> An earlier version of this section quoted 588,588 at turn 4, 4,119,876 at turn
> 5 and 28,840,812 at turn 6 for "a 99-card deck with 38 lands in five mana
> profiles", and named no deck. The first and third are the compositions a
> **seven**-group question costs at those turns on the play, and the middle one
> is not a number any grouping produces — seven groups at turn 5 is 4,120,116,
> which is 588,588 × 7, the factor one more checkpoint adds. The deck was never
> recorded and nobody has reproduced it since, so the figures above replace it
> with two decks that are in this repository.

**One land drop, one declared policy.** A `[[effect]]` that routes cards and a
mana question are both answers to *which land did you play this turn*, and left
alone they answer it differently: the effect plays the deepest-looking land you
hold, the gate assumes whichever land pays. Neither of those is a fact about
your deck — it is a decision you make every game — so you declare it, and both
halves then read the same drop:

```toml
[land_drop]
prefer = ["t:land otag:surveil", "t:land -otag:tapland"]
```

The list is read in order and the **first entry a land in hand matches wins**.
A land the list does not name is played **last**, not never: a priority list is
a preference, and a drop you decline is a drop you never get back. A **tie**
inside one entry goes to the deeper look — so a list that does not mention your
surveil land still fires the surveil — and then to the card your decklist names
first.

This is the same mechanism as mulligan bottoming
([#7](https://github.com/cramt/progress-engine/issues/7)) and selection routing:
a declared priority over queries, evaluated against counts. There is not a
fourth policy language and there will not be one.

**Every run that resolves a land drop this way says so**, beside the
tapped-ness assumptions and for the same reason — a number that turned on a
choice has to name the choice:

```
note: the land drop here is decided by the priority this file declared, and every
      number below that depends on which land was played depends on it:
      1. "otag:surveil"
      then any other land. Ties: a tie inside one entry goes to the deeper look, then to the
      card this decklist names first.
```

The same list is in the JSON as `land_drop`. A file that declares none has no
`land_drop` field at all, which is a different fact from an empty one: nothing
in that run needed the drop arbitrated.

**Declare nothing and hold both, and it is still refused** — with the remedy
named rather than a default chosen, because *which land would you have played*
is a question only you can answer:

```
$ progress-engine test lantern.txt lantern.criteria.toml
Error: lantern.criteria.toml: Lantern castable on turn 1: a mana question and a live land-drop
      effect are both answers to which land you played this turn, and this file declares no
      priority between them. [...]
      Declare the priority and both read the same drop:

      [land_drop]
      prefer = ['otag:surveil', 't:land -otag:tapland']
```

**What it costs.** Nothing, unless the list draws a line nothing else drew. The
preferences become grouping queries, so an entry that separates lands the mana
model or an effect already separated is free, and one that splits a group
nobody else split costs a group — exactly as `can_cast` does. Measured on a
narrower manabase than the one above, so the figures are not comparable with
it: 99 cards, 38 lands in five printings but only three mana profiles — one
blue, one black, one tapped dual — asking `can_cast = "{1}{U}"`:

| Turn | no policy | `t:land -otag:tapland -otag:conditional-tapland` (4 groups) | plus a `t:land otag:surveil` tier (5 groups) |
|---|---|---|---|
| 4 | 7,680 — 0.15s | 7,680 — 0.15s | 41,250 — 0.25s |
| 5 | 30,720 — 0.21s | 30,720 — 0.21s | 206,250 — 0.74s |
| 6 | 122,880 — 0.47s | 122,880 — 0.49s | 1,031,250 — 3.21s |
| 7 | 491,520 — 1.56s | 491,520 — 1.64s | 5,156,250 — over the ceiling, sampled |

The first list is free because tapped-ness is a line the gate already draws. The
second costs a group because nothing else told a surveil land from any other
tapland — *unless the file also routes with it*, and then that group exists
already: the same surveil tier beside a live `to_graveyard` effect is 25,781,250
compositions across 5 groups with or without it, to the path. Which is the case
this feature exists for.

A list is still read in a run that observes no land drop at all — no mana
question and no live effect — and there it costs groups and moves no number,
because nothing in that run can tell which land you played. Declare one where
something reads it.

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
Preordain, tutors) is still refused by name, and the gate shipping does not
change that: an opening hand of one Island and six Opt *can cast* Opt and casts
exactly one of them, because the first one spends the Island. Firing an effect
whenever the mana is there overstates that turn sixfold. That needs the budget
half of [#10](https://github.com/cramt/progress-engine/issues/10).

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

**`--from` builds an index with no oracle tags, and that costs you the effect
library.** Tags are not in the bulk data — they come from Scryfall's search API,
which `--from` by definition does not call — so an index built that way carries
none. Everything keyed on `otag:` is therefore inert on it: the
[standard effect library](#effects) is keyed entirely on `otag:`, so
it matches nothing, applies nothing and reports `"effects": []`. `sync` says so
when it builds one, and a run against one says so again rather than answering an
`otag:` question with a confident zero
([#50](https://github.com/cramt/progress-engine/issues/50)). `--from` is for
tests and offline machines; a working index comes from a plain `sync`.

**A tag that fails does not cost the download.** The bulk file is 25MB and the
six tag searches are about forty small requests, so the cheap half is the flaky
one: a single HTTP 429 twenty-three pages into the sixth tag used to fail the
whole command and throw away a download that had already parsed. Now a
rate-limited request is retried with an exponential backoff — 1, 2, 4, 8
seconds, or whatever `Retry-After` asks for — and the gap between requests
widens for the rest of the run after the first refusal. A tag that still cannot
be fetched costs that tag: the index is written with the tags that succeeded,
its header lists exactly those, the ones that failed are named, and the command
exits non-zero so nobody mistakes it for a full sync. Running `sync` again
finishes the job, and does not report the index as already current while a tag
is missing.

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

`sync` fetches seven tags today, each because something here reads it:

| Tag | Cards | Read by |
|---|---|---|
| `tapland` | 495 | whether two lands are actually two mana |
| `conditional-tapland` | 179 | the half `tapland` is not: a shockland enters tapped only if you decline to pay, so the gate has to say which way it read the choice |
| `surveil` | 334 | how many cards deep a turn sees |
| `scry` | 475 | the same, leaving the card on top |
| `mill` | 1,305 | the graveyard as a destination |
| `tutor` | 1,168 | selection over the whole library |
| `ramp` | 2,316 | the mana-curve questions |

They cost nothing to carry: about 5,400 of 35,486 cards are tagged, the file is the
same 24MB, and a run parses only the cards your deck names either way.

**An index carries the tags it was told to fetch, and says which.** The header
lists them, so `otag:` can tell *this index never asked about that tag* apart
from *no card is in it* — the same empty result, and very different facts. A
query naming a tag the index does not carry is an error naming the tag, for the
same reason `kw:tramp` is.

There are three ways an `otag:` question comes back with no cards, and they are
three different answers:

```
$ progress-engine test loam.txt mill.criteria.toml
Error: a mill card in the opener: in query "otag:mill": this index does not carry otag:mill, so counting it would be zero by construction
      rather than by measurement. This index carries: scry, surveil, tapland.

$ progress-engine test simple-ramp.txt surveil.criteria.toml     # an index built with --from
Error: surveil lands in the opener: in query "t:land otag:surveil": this index carries no oracle tags at all, so otag:surveil would match nothing here
      whether or not this deck plays such a card.
      `sync --from` builds an index like this one: tags come from Scryfall's search API,
      not from the bulk file. Fetch them with: progress-engine sync

$ progress-engine test loam.txt scry.criteria.toml
note: query "otag:scry" matched no cards in this deck
```

Only the third is an answer, and only the third is a fact about the deck. The
second used to be the first two silently behaving like the third
([#50](https://github.com/cramt/progress-engine/issues/50)): the index that
`--from` builds answered every `otag:` question with a confident zero and
switched the whole effect library off on the way past. The same distinction
reaches the effect library, which is not refused — nobody asked for it — but
does say when this index is the reason it matched nothing.

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

A keyword is checked against the index rather than against the parser, because
the set of real keywords grows with every set and a list hard-coded beside the
parser would start refusing real queries the day it fell behind. So `kw:flyign`
is refused where `cat:"Rmap"` above could only be noted — the index knows every
keyword the card pool carries, and a typo among them is that confident 0%
wearing a valid query:

```
$ progress-engine test loam.txt kw-typo.criteria.toml --index loam-index.jsonl
Error: a typoed keyword: in query "kw:flyign": no card in this index has kw:flyign, so counting it would be zero by construction
      rather than by measurement — which reads exactly like a deck that plays none. The
      index lists every keyword the whole card pool carries, so this is a misspelling
      unless it is newer than the index. Check the spelling; rebuild with: progress-engine sync
```

An index whose header never listed any keywords refuses nothing. Keywords are
derived from the cards, so an index that never wrote them down is silent rather
than authoritative, and silence is not evidence that any keyword is unreal —
unlike oracle tags, which are fetched, and where carrying none is a fact the
header states about itself.

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
