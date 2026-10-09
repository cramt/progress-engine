# progress-engine

Magic: The Gathering tooling, named for Jin-Gitaxias's faction of New Phyrexia
on the grounds that obsessively recalculating whether your deck is perfect yet
is blue-aligned behaviour. [NAMES_FOR_FUTURE.md](NAMES_FOR_FUTURE.md) carries
the rest of the family and the rule for naming the next one. Three products
live here:

- **Ichormoon Gauntlet** (`crates/ichormoon-gauntlet/`, binary `gauntlet`):
  draw-probability tests for decklists, answered by exact enumeration. The rest
  of this README is about it.
- **Meldweb Curator** (`crates/meldweb-curator/`, at
  [meldweb.cramt.dk](https://meldweb.cramt.dk)): a deck and collection editor
  in the browser where a `.deck.toml` in your own git repo is the deck and
  saving is a commit.
- **Gitaxian Probe** (`crates/gitaxian-probe/`): card scanning, hosting
  [Delver X](https://mtg.delver.app)'s recognition engine natively and in the
  browser, where Curator's Scan button uses it. Its
  [engine README](crates/gitaxian-probe/engine/README.md) is the authority on it.

Gauntlet and Curator stand on **Reality Chip** (`crates/reality-chip/`), the shared core:
card data and Scryfall's query syntax, decklist parsing, and the
hypergeometric walk. See [Crate layout](#crate-layout).

# Ichormoon Gauntlet

Draw-probability tests for Magic: The Gathering decklists. A gauntlet is a set
of trials you put something through, which is what a criteria file is.

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
into classes and each class gets the cheapest enumeration that can answer it. Three things
shrink:

- **Groups.** A class keeps only the queries it reads, plus whatever the walk itself reads —
  which cards a live effect applies to, where it routes them, the land-drop priority, the
  casting priority — and keeps what a land makes only if something asks whether a cost could
  be paid. Everything else merges. The budget is the one that reaches every class: a spell it
  paid for is a spell that left the hand, so a criterion counting `cat:"Ramp"` beside a
  declared `[casting]` list depends on the manabase whether or not it ever mentions one. A
  file that declares none pays nothing for it.
- **Colours** ([#55](https://github.com/cramt/progress-engine/issues/55)). A cost can only
  tell apart the colours it demands. `Cost::payable` runs Hall's condition over the pip kinds
  in the cost and nothing else, so to `{1}{U}` a Plains, a Swamp and a Forest are one source:
  each pays a generic and none pays the pip. So the manabase is keyed on the palette
  **intersected with what the class's costs demand**, which is at most four land groups for a
  one-colour cost however many printings are behind them. See [what castability
  costs](#mana-as-a-gate) for the case this exists for, and for the one run where it is
  refused.
- **Checkpoints.** A class keeps only the turns it names. "Loam in hand by turn 5" is one
  multivariate hypergeometric over eleven cards, not a path through five checkpoints. A
  criterion that correlates two turns names both and keeps both, because the hands that get
  there late are exactly the ones it excludes; a question about what is on the battlefield or
  about paying a cost keeps every turn up to its own, because one land drop a turn is
  use-it-or-lose-it and no total can say that.

Measured on the two decks in `decks/`, against the tagged index committed beside them. The
first four rows are what #31 bought and the last two are #55, so *before* means before that
row's own narrowing:

| File | Before: groups / compositions | After: widest class | Before | After |
|---|---|---|---|---|
| `lantern.criteria.toml` | 16 / 11,176,771,584 | 18 / 36,332,613,504 | sampled | sampled, 2.2s |
| `lantern.criteria.toml --draw` | 16 / 178,828,345,344 | 18 / 653,987,043,072 | sampled | sampled, 2.4s |
| `loam-access.criteria.toml`, then `loam.criteria.toml` | 8 / 14,057,472 | 6 / 1,026,432 | **sampled** | **exact**, 3.9s |
| `loam-access.criteria.toml --draw` | 8 / 112,459,776 | 6 / 6,158,592 | sampled | sampled, 7.9s |
| `lantern.deck.toml`, `can_cast = "{1}{U}"` at turn 4 | 17 / 1,204,456,341 | 5 / 41,250 | **sampled**, 0.68s | **exact**, 0.27s |
| `loam.deck.toml`, the same clause | 18 / 2,018,478,528 | 5 / 41,250 | **sampled**, 0.81s | **exact**, 0.25s |

The first two rows are what narrowing looks like once a file asks the question it exists for
rather than a proxy for it. `lantern.criteria.toml` was then the Lantern north star written as a
union of four routes (it is one line now, [#100](https://github.com/cramt/progress-engine/issues/100)), eleven of whose fourteen branches price a cost, and no narrowing gets
that under the ceiling: it is over 7,000 times over. (It was 12 groups until Artificer's
Intuition's discard became a `t:artifact` clause; the *Before* column predates that.) What #31 buys there is **the other
thirteen questions in the file**, which are enumerated exactly and cost between 8 and 122,880
compositions apiece — before #31 every one of them would have been estimated on account of the question
beside it. The run says which four were not, by name, and quotes only those with a ±.

Every number that was exact before is the same number after, to every digit the report
prints. Narrowing is not an approximation: a coarser grouping is a marginal of the finer one,
the draws between two turns nobody reads have the same joint distribution merged as separate,
and two lands a cost cannot tell apart are one source to the matching it runs. All three are
asserted as properties over generated questions, and end to end through the binary against the
sampler — which walks the un-narrowed grouping and so is a second implementation rather than
this one agreeing with itself.

**A run says how it enumerated**, because a file is several enumerations and a figure quoted
without its width is a figure nobody can reproduce. Every run's JSON carries an
`enumerations` block, one entry per class: the questions it answered, the queries and turns it
reads, the pips it kept, its group and composition counts, and whether it was walked or
sampled. Every group and composition count in this README comes from one of those, and the
command that produces it is printed beside it.

### Threads

A walk is split at its opener: each opening hand's rest of the game is its own
walk, sharing nothing with any other, so they are handed out to every core the
process may use and summed on one thread afterwards, in a fixed order. That
order is why **the number of threads never reaches a digit** — one thread and
sixty-four print the same bytes, and a test holds them to it. Set
`GAUNTLET_THREADS` to use fewer; it changes how long a run takes and nothing
else.

On the four-core machine these figures were measured on, the Loam optimiser run
above went from 11.0s to 2.5s, a declared mulligan on the same deck from 7.5s to
1.6s, and a plain run of `decks/loam-access.criteria.toml` (then
`loam.criteria.toml`) on the draw from 1.5s to 0.5s — every one of them byte-identical to the single-threaded run, which is
byte-identical to what the tool printed before. Some of it is not threads at
all: `ln C(n, k)` is read off a table of the exact values `lgamma` returns
rather than recomputed millions of times, and a path no longer allocates.

SIMD was tried and measured rather than assumed, with `fearless_simd` on the one
loop shaped for it — summing a query's cards across groups — and it lost: a run
that took 7.4s took 9.5s, because a query touches one to three groups and the
dispatch costs more than the adding. What is left of a run is the per-path walk
of the board, which branches on every card, and that is the shape vector units
and GPUs are worst at.

### When the question is too wide

A question that is still over the ceiling on its own — a criterion correlating two turns
across seven queries, say — used to be refused. It is now **answered by sampling, loudly**:

```
$ gauntlet test simple-ramp.txt too-wide.criteria.toml
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
$ gauntlet test simple-ramp.txt too-wide.criteria.toml --exact
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
| `gauntlet sync` | Build the card index from Scryfall bulk data |
| `gauntlet parse <deck>` | The canonical decklist parser, as JSON; a `.deck.toml` is named from the index |
| `gauntlet import <deck.txt>` | An Archidekt export as a `.deck.toml`, on stdout |
| `gauntlet test <deck> <criteria.toml>` | Evaluate criteria and report PASS/FAIL |

```
$ gauntlet test simple-ramp.txt simple-ramp.criteria.toml
note: every number below keeps whatever seven it is dealt: this file declares no
      [mulligan], so no hand is ever sent back.
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

widest exact question: 108 compositions across 3 groups, under 0.01% of the 5,000,000 ceiling, "commander on turn 2"

PASS: 3 of 3 assertions met
```

The line above the verdict is how close the run came to the enumeration
ceiling: the widest question it walked exactly, and its share. It is on every
run so that the ceiling is visible before a question crosses it — Route B's
turn-5 line on `decks/lantern.deck.toml` sits at 82% of it, one more card or turn
from being estimated.

JSON goes to stdout, the verdict to stderr, and the exit code reflects it — so a
caller piping stdout through `jq` cannot lose the failure.

A criterion with no `at_least` and no `at_most` is informational: it reports a
number and cannot fail. A file with no bound anywhere still exits 0, but its last
line says `NOTHING ASSERTED` rather than `PASS`, because it tested nothing. The two blocks with means under them are `[[expect]]` rather than
`[[criterion]]`, and they are the subject of [how many, not just how
often](#how-many-not-just-how-often). Add `--draw` to model being on the draw,
`--simulate` to sample instead of enumerate (slower, approximate, and reported
with standard errors), and `--exact` to refuse a question too wide to enumerate
rather than [estimating it](#when-the-question-is-too-wide).

The JSON also carries an `enumerations` block — one entry per class of question,
saying what that class reads, how wide it was, and whether it was walked or
sampled:

```
$ gauntlet test simple-ramp.txt simple-ramp.criteria.toml | jq -c '.enumerations[]'
{"criteria":["keepable opener (2-5 lands)"],"expectations":["lands in opener"],
 "queries":["t:land"],"turns":[0],"reading":"cumulative","groups":2,
 "compositions":8,"method":"exact"}
...
```

A file is several enumerations rather than one — see [one enumeration per
question](#one-enumeration-per-question-not-per-file) — so *how wide was this*
has an answer per question, and without this block the only one that reached
the output was the widest class the run **refused**. It is there for the same
reason the provenance block is: a figure whose inputs are not named cannot be
reproduced or compared, and that includes the figures about the figures.

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
an optional `at_least` and `at_most`, and `require`: a list of clauses, all of
which must hold. A clause asks one of three things, told apart by which key it names: a
`turn`, a `query`, an optional `zone` and at least one of `min` and `max`
counts cards; a `turn` and a `can_cast` asks whether a cost was payable, for
which see [Mana, as a gate](#mana-as-a-gate); and a `turn`, a `cast` and at
least one of `min` and `max` counts the spells you paid for, for which see
[Mana, as a budget](#mana-as-a-budget). A criterion can also hold `any_of`, a
list of alternative routes, for which see [Routes](#routes-any_of). An
`[[expect]]` is a `name`, a `turn` and either a `query` with an optional `zone`
or a `cast`, and reports a distribution rather than a verdict. A file may also
hold `[[effect]]` tables, for which see [Effects](#effects), one `[land_drop]`
table saying which land you would play when you could play either, one
`[casting]` table saying which spells you would cast when the mana cannot pay
for all of them, and one `[mulligan]` table saying which openers you keep and
what you put back, for which see [Mulligans](#mulligans).

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

`at_least` is a floor and `at_most` a ceiling, and both are shares of hands.
A ceiling is for the number that should stay small, because a probability
drifting *up* is sometimes the regression:

```toml
[[criterion]]
name = "flooded opener (6+ lands)"
at_most = 0.15
require = [{ turn = 0, query = "t:land", min = 6 }]
```

Write both and the criterion is a range. A missed range says which end it
missed, `(needs 40.0% to 60.0%: over it)`, and the JSON carries the same thing
as `missed: "at_least"` or `"at_most"` on every failed criterion. Both bounds
are inclusive.

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
[Mana, as a gate](#mana-as-a-gate) below, and whether you actually did is
[Mana, as a budget](#mana-as-a-budget).

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
| `hand` | the default. Cards drawn by that turn, less what the [`[casting]` line](#mana-as-a-budget) cast and what a [discard](#discard-a-spell-that-draws-and-then-bins) binned. A land counts from the turn it is drawn, played or not |
| `graveyard` | cards an effect routed, milled or discarded to the yard — see [Effects](#effects) — and every instant or sorcery the [`[casting]` line](#mana-as-a-budget) cast, which resolves into it. Zero in a run where neither can happen, and the run says so |
| `library` | cards matching the query that are still in the deck: the deck's total minus what has been drawn or binned |
| `battlefield` | **lands you have played**, one drop a turn — the ones your `[land_drop]` priority played, or the most you could have played if you declared none — plus every permanent the [`[casting]` line](#mana-as-a-budget) cast and anything a [delayed effect](#delayed-effects-urzas-saga) or a [cast fetch](#a-cast-that-puts-a-card-onto-the-battlefield-tezzeret-the-seeker) put there. A permanent still in hand is not on it, declared land drop or not ([HANDS.md](HANDS.md) hand 43). Refused for any other card that would have to be cast |

`graveyard` is reachable exactly when some effect in the run routes a card
there, or the declared casting line names an instant or a sorcery — a spell
that is not a permanent is put into its owner's graveyard when it resolves, so
casting Life from the Loam is a route to the yard. In a run where neither
happens, every count in it is zero by construction —
which is a confident zero, the failure this whole tool is about — so the run
says so rather than letting `0.00%` pass for a measurement:

```
$ gauntlet test loam.deck.toml loam.criteria.toml
note: nothing routes a card to the graveyard in this run, so every count in
      it is zero by construction rather than by measurement.
      Asked by: "loam in the yard by turn 5"
      Declare `to_graveyard` on an [[effect]] to route one there, or name an
      instant or sorcery in [casting]: one the line casts resolves into it.
     loam in the yard by turn 5    0.00%
```

Declare a destination, or a line that casts one, and the note goes away,
because the number is now a measurement. `decks/loam-cast.criteria.toml` was
the second kind, until #101 folded it into `decks/loam.criteria.toml`: it
declared a line that casts Life from the Loam and asked the north star of the
zone. Casting Loam alone, it read 9.9210% on the play — the
same hands as `can_cast` of Loam by turn 5, to the digit, because one copy cast
the first turn it is payable is in the yard exactly when it was payable
(HANDS.md hands 34 and 35). The line then cast Spellseeker too, which fetches
the Loam (HANDS.md hand 36, and [Tutors](#tutors-and-a-library-that-shrinks)),
and read 17.08% ± 0.08 on the play and 19.58% ± 0.09 on the draw — sampled,
because Spellseeker's `{2}{U}` makes the manabase tell blue from green and the
question 284,738,168 compositions wide. With the four mills in the line as
well it read 19.02% ± 0.09 and 21.82% ± 0.09
([Mills](#mills-a-spell-that-turns-cards-over)), with the discards and a
declared land drop 19.53% ± 0.09 and 22.14% ± 0.09
([Discard](#discard-a-spell-that-draws-and-then-bins)), and with Six, Icetill
Explorer and Lumra, whose mills are an attack's, a landfall's and an enters
trigger's, it read 19.79% ± 0.09 and 22.57% ± 0.09
([Attack and landfall](#attack-and-landfall-a-mill-that-fires-again)). That line, with the commander
and the dorks in it, is now the north star's ([The Loam north star](#the-loam-north-star-one-line)),
and the questions that ask no line moved to `loam-access.criteria.toml`, because a `[casting]` line
takes the cards it casts out of the hand and prices the manabase on every question beside it.
Flashback and retrace, which cast a card *from* the yard, are not modelled.

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
| `at_least = 0.6, at_most = 0.4` | no probability sits between them, so it fails every deck and blames the deck for it |
| `zone = "battlefield"` on a query matching a spell | a land arrives on a land drop and this engine walks those; a spell has to be cast, and where it goes afterwards is not modelled. `cast` counts the castings, which is the part that is known. The exceptions are the three ways this walk models a permanent arriving: the `[casting]` line casting it, and a [delayed effect](#delayed-effects-urzas-saga) or a [cast fetch](#a-cast-that-puts-a-card-onto-the-battlefield-tezzeret-the-seeker) putting it there. An instant or sorcery stays refused, because it resolves into the graveyard — ask `zone = "graveyard"` for it |
| `zone = "exile"`, or any other zone | a zone that fell through to a default would answer the wrong question |
| two of `query`, `can_cast` and `cast` in one clause | three different questions, two of which would have to be answered silently |
| `cast` and `zone` in one clause | a casting is not a zone. Where the spell is afterwards is a `query` with a `zone`: a cast instant or sorcery in the graveyard, a cast permanent on the battlefield |
| `cast` with no `[casting]` table | which spell you cast out of one turn's mana is a decision, and a tool that picked would report a line nobody chose |
| `[casting]` naming a card whose cost holds `{X}`, hybrid or no symbols at all, and whose effect declares no `cost` | a bill read too cheaply does not only get that spell wrong — it leaves mana the rest of the line then spends. [Declare what you pay](#a-cost-the-line-pays-that-is-not-printed-dizzy-spell-and-whir-of-invention) and the `{X}` is yours |
| `cost` holding `{X}` or a hybrid symbol, or on an effect that is not `on = "cast"` | a declared cost is the amount paid, already chosen; and a land drop pays nothing |
| `can_cast = "{X}{G}"`, or any hybrid or Phyrexian symbol | each is a decision about how much to pay rather than an amount; read as zero, an X-spell is castable on turn one |
| a mana question in a file with a live `to_graveyard` effect and no `[land_drop]` | both decide which land you played this turn, and they decide it differently. Declare the priority and they are one decision |
| `[land_drop]` or `[casting]` with no `prefer` entries | it settles nothing, and the run would report a policy that decided nothing |
| a `prefer` entry repeating an earlier one | the earlier one already took every card it names, so it can never decide anything |
| `atLeast`, or any other unknown key | a key quietly dropped is an assertion quietly deleted |
| a file with no `[[criterion]]` and no `[[expect]]` | it asks nothing |
| `look` on an `on = "cast"` effect | the budget knows you cast it; what it does not know is what casting it drew, and a replacement draw is over the enumeration ceiling on every question this tool exists for ([#57](https://github.com/cramt/progress-engine/issues/57)). A `fetch` on a cast is answered |
| `fetch` with no `to`, or `to` with no `fetch` | half a declaration, and half a declaration is where a default nobody stated gets invented |
| `fetch ... to = "battlefield"` on an `on = "cast"` or `"activate"` effect whose priority matches a land, in a file with no `[land_drop]` | a land a spell puts down is counted beside the lands played, and only a declared drop records which those are (ADR-0025). With one declared, it is answered: the land is read as entering tapped, and pays from the next turn |
| `fetch ... to = "battlefield"` on an `on = "landdrop"` effect, naming a card that is not a land | a fetchland finds a land; anything else has to be cast, or put there by a cast or a delayed effect |
| `fetch ... to = "battlefield"` on a cast or delayed effect whose priority matches an instant or a sorcery | it is not a permanent, and nothing can put it onto the battlefield |
| a `fetch` beside a `can_cast`, a `cast` or a `[casting]` table, where it puts a land onto the battlefield | what a fetched land taps for on the turn it arrives is a fact about the spell that fetched it, and `otag:fetchland` holds both kinds |
| `on` anything else, or `look = 0`, or an effect that neither looks nor fetches | a trigger nothing fires, and an effect that cannot move a number |
| `after` on an `on = "cast"` effect that does not `adds`, or beside a `look` | a delayed look turns over cards on a turn the schedule cannot know in advance, and a cast leaves nothing in play to wait with but a rock or dork, whose `after` is how long it adds nothing |
| `sacrifice = true` beside `adds` | a rock that waits is a rock entering tapped, and nothing is sacrificed when the wait is over |
| `after = 0`, or more than 100 | an effect that waits no turns is written without `after` |
| `adds` on a card whose card data produces no mana, such as Wood Elves | a source taps for the colours its card produces, so this one would count nothing while the run said it applied. A card that searches for a land is a `fetch` |
| `sacrifice = true` with no `after` | a land that sacrifices itself the moment it is played is a fetchland, and is already written `to = "battlefield"` |
| a delayed `fetch ... to = "battlefield"` whose priority matches a land | a land arriving off an ability is not a land drop, and whether it enters tapped is a fact no tag carries |

Every one of those messages names the file, the question, and what was wrong
with it.

### Mana, as a gate

Two lands is not two mana. A land that enters tapped makes none the turn it
arrives, a land you drew on turn four is not a land you played on turn two, and
one Hallowed Fountain is a white source and a blue source and one mana.

So there are two questions here, and this is the first one:
**could I have paid for it by turn N?** The other is mana as a *budget* — an
opening hand of one Island and six Opt casts one Opt, because the first one
spends the Island — and that is [the next section](#mana-as-a-budget).

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
      A shockland's 2 life is the pilot's decision, so the pessimistic reading is taken: it makes
      no mana the turn it arrives. Every number below that depends on one of these is a
      floor rather than a measurement. Declare the ones you play untapped with
      `[assume] untapped = ['name:"Breeding Pool"']`.
```

The same list is in the JSON as `assumed_tapped`. It understates a real
shockland manabase, which is the direction this tool prefers to be wrong in: a
number your deck can beat is a better failure than one it cannot reach.

The file can settle the condition instead. `[assume] untapped` is a list of
queries, and every conditional tapland one matches is read as entering untapped:

```toml
[assume]
# Battlebond lands: untapped in any game with two or more opponents.
untapped = ['o:"two or more opponents"', 'name:"Breeding Pool"']
```

The run prints the lands it read so, and the JSON carries them as
`declared_untapped`. Only a conditional tapland can be declared: a query that
matches a land Scryfall tags `otag:tapland`, such as a guildgate, is refused by
name, and one that matches no conditional tapland is noted as declaring nothing.
Nothing reads the condition itself. The tool does not know how many opponents
the game has (it is format-agnostic, ADR-0005), and a checkland's "unless you
control a Forest or an Island" would need every land's basic types in the
grouping, which no question pays for today. So declaring a checkland untapped
is the pilot's claim that the condition holds, and the run prints it as one.

**Lands that make other than they list.** Scryfall's `produced_mana` is every
kind of mana a card *could* make, with conditions ignored, and nothing for a
land whose mana is the land it fetches. Taken at its word it overstates some
lands and understates others, so each land is read once, against the rules and
against the deck it sits in, and **every run that prices mana names the ones it
read other than at face value**, on stderr and as `assumed_mana` in the JSON:

```
note: these lands make mana other than the card data lists, and this run reads them as:
      Castle Doom: pays {C} only: its other colours come with a condition this engine cannot see.
      Maze of Ith: makes no mana: it has no mana ability, so it is a land drop and pays for nothing.
      Misty Rainforest: a fetchland, read as the untapped lands it can find in this deck (Forest,
      Island, Taiga, Tropical Island, Volcanic Island): pays {U}{R}{G}, assuming one is still in
      the library to find.
      Urza's Saga: makes mana for three turns, the one it is played on and the two after: its
      last chapter sacrifices it.
```

| Land | Read as | Why |
|---|---|---|
| A fetchland (Misty Rainforest, Prismatic Vista) | the colours of the **untapped** lands its search can find in this deck; a tapped land of every colour it can find if it says "tapped" (Evolving Wilds) | the land it finds pays the turn it is cracked. **Assumes one is still in the library**, which ignores running out — a ceiling, named. HANDS.md hand 38 |
| A land with no mana ability (Maze of Ith) | a land drop that pays nothing, not even generic | HANDS.md hand 39 |
| Castle Doom, Spire of Industry | `{C}` only | their colours need an artifact spell (CR 106.6) or an artifact in play |
| Exotic Orchard | generic, no colour | its colour is whatever an opponent's lands could make (CR 106.7) |
| A Saga land (Urza's Saga) | mana on the turn it is played and the next two | its last chapter sacrifices it (CR 714.4), with or without the effect declared |
| A bounce land (Izzet Boilerworks) | one mana a turn | its second mana and the land it returns are **not modelled**, and named |

A conditional palette is found in the oracle text — a mana ability saying
"Spend this mana only", "Activate only if" or "could produce" — and the land
keeps only the colours its other mana abilities make. A fetchland is a land
Scryfall lists as making nothing whose text sacrifices it to search for land
types, without mana in the cost.

**What it costs.** Maze of Ith and a Saga are each a land no other land is
interchangeable with, so a class that prices mana on a deck holding them keeps
up to two more land groups, even for a cost naming no colour. On
`decks/lantern.deck.toml` that took *route 1* — Lantern in hand and `{1}` by turn 5 —
from 4 groups to 6: still exact on the play at 1,026,432 compositions, and 23%
over the ceiling on the draw, where it is now sampled.

**What it costs.** Counting lands in play is free — it reads the land drops the
enumeration already walks and adds no group and no path. `can_cast` is not free,
but it is far cheaper than it was. A cost can only tell apart the colours it
demands, so the manabase is keyed on *(the pips this class's costs demand, does
it arrive tapped)* rather than on the whole palette: for `{1}{U}` that is at most
four land groups — makes blue untapped, makes blue tapped, makes something else
untapped, makes something else tapped — however many printings sit behind them.

Measured on the decks in `decks/`, against an index synced with oracle tags,
asking `can_cast = "{1}{U}"`, before and after
[#55](https://github.com/cramt/progress-engine/issues/55):

| Deck | Turn | Before: groups / compositions | After | Before | After |
|---|---|---|---|---|---|
| `lantern.deck.toml` | 4 | 17 / 1,204,456,341 | 5 / 41,250 | sampled, 0.68s | **exact**, 0.27s |
| `lantern.deck.toml` | 5 | 17 / 20,475,757,797 | 5 / 206,250 | sampled, 0.76s | **exact**, 0.74s |
| `lantern.deck.toml` | 6 | 17 / 348,087,882,549 | 5 / 1,031,250 | sampled, 0.84s | **exact**, 3.2s |
| `lantern.deck.toml` | 7 | 17 / 5,917,494,003,333 | 5 / 5,156,250 | sampled | sampled — over the ceiling by 3% |
| `loam.deck.toml` | 4 | 18 / 2,018,478,528 | 5 / 41,250 | sampled, 0.81s | **exact**, 0.25s |
| `loam.deck.toml` | 6 | 18 / 653,987,043,072 | 5 / 1,031,250 | sampled, 0.86s | **exact**, 3.1s |

Reproduce any row with the `enumerations` block:

```
$ gauntlet sync --index /tmp/index.jsonl
$ printf '[[criterion]]\nname = "u"\nrequire = [{ turn = 4, can_cast = "{1}{U}" }]\n' > /tmp/u.toml
$ gauntlet test decks/lantern.deck.toml /tmp/u.toml --index /tmp/index.jsonl | jq -c '.enumerations[]'
{"criteria":["u"],"expectations":[],"queries":[],"turns":[4],"reading":"per-turn",
 "pips":["{U}"],"groups":5,"compositions":41250,"method":"exact"}
```

Note where the time went: sampling 200,000 hands is about 0.8s whatever the
question, so an exact answer at turn 6 is *slower* than the estimate it
replaces. That is the trade this makes on purpose — the matching itself is
cheap, about 0.65µs a hand, and the width is what costs.

Five groups is the ceiling for a clause that asks only a one-colour cost — four
land groups and everything else — so `{1}{U}` alone is exact through turn
**six** on any deck whatever its manabase, and goes over at turn seven by 3%.
Two colours in one class is at most eight land groups, and the join is per
class: two criteria asking different costs are two enumerations, each narrowed
to its own. A clause that also counts something keeps that query's bit as well,
exactly as it did before.

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

> **Two things measured about that tie, and one of them is a known gap**
> ([#100](https://github.com/cramt/progress-engine/issues/100)), found by
> holding the engine to `checker/` on small decks. A look that routes nothing —
> a surveil land in a file that declares no `to_graveyard`, whose card stays on
> top — is no deeper than no look, and breaks no tie: Hedge Maze in
> `decks/lantern.deck.toml` is played in decklist order. And where one entry holds
> lands that make different mana, the engine breaks the tie by **land group**
> rather than by card: lands that make the same mana the same way are one
> group, ranked where the first of them sits in the list. With Training
> Center, Tropical Island and Turbulent Springs in one entry, Turbulent Springs
> — the same `{U}{R}` tapped land as Training Center — is played before
> Tropical Island, where the rule above plays Tropical Island. On the whole
> Lantern deck, *Saga, then every tapped land, then any land* reads the north
> star at 29.43% / 35.87% where the stated rule reads about 29.8% / 36.2%. It
> is the unsoundness `a_declared_priority_is_not_allowed_to_merge_two_lands_it_ranks_apart`
> guards against for palettes, one step further, and it is filed rather than
> fixed. A list with one kind of land per entry, as
> `decks/lantern.criteria.toml` declares, is not affected.

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
$ gauntlet test lantern.deck.toml lantern.criteria.toml
Error: lantern.criteria.toml: Lantern castable on turn 1: a mana question and a live land-drop
      effect are both answers to which land you played this turn, and this file declares no
      priority between them. [...]
      Declare the priority and both read the same drop:

      [land_drop]
      prefer = ['otag:surveil', 't:land -otag:tapland']
```

**What it costs, beside a mana question: the colour narrowing.** This is the
price and it is not small. A priority plays the first land in hand that its list
reaches, and the tie inside one entry goes to *the card your decklist names
first* — so it reads the manabase for a reason no cost can state. Two lands
that make the same mana the same way, and that no query in the file tells
apart, are one card to the run (a Forest and Boseiju, Who Endures), so the tie
is between kinds of land and goes to the kind the decklist names first. Merging a
Plains with a Swamp because `{1}{U}` cannot tell them apart would renumber that
ranking and play the wrong land, so a run that declares a priority keeps the
whole palette and pays for it. Measured on `decks/lantern.deck.toml` — a 99-card
library whose 40 land-typed cards make sixteen distinct mana profiles — asking
`can_cast = "{1}{U}"`:

| Turn | no policy (5 groups) | `t:land -otag:tapland -otag:conditional-tapland` (17) | plus a `t:land otag:surveil` tier (19) |
|---|---|---|---|
| 4 | 41,250 — exact, 0.26s | 1,204,456,341 — sampled | 3,297,121,300 — sampled |
| 5 | 206,250 — exact, 0.74s | 20,475,757,797 — sampled | 62,645,304,700 — sampled |
| 6 | 1,031,250 — exact, 3.21s | 348,087,882,549 — sampled | 1,190,260,789,300 — sampled |
| 7 | 5,156,250 — sampled | 5,917,494,003,333 — sampled | 22,614,954,996,700 — sampled |

So on this deck the priority costs the exact answer outright, and the two lists
cost about the same as each other: the untapped tier separates nothing the mana
model had not already separated, and the surveil tier adds two groups. Recovering
the narrowing under a declared priority — by merging only lands the ranking
already places side by side — is
[#56](https://github.com/cramt/progress-engine/issues/56), and until it exists
the honest thing is the number the run actually walked.

**What it costs where nothing prices mana.** Nothing, unless the list draws a
line nothing else drew. The preferences become grouping queries, so an entry
separating lands an effect already separated is free, and one that splits a
group nobody else split costs a group. The case this feature exists for is the
free one: a surveil tier beside a live `to_graveyard` effect on `lantern.deck.toml` is
20 groups and 841,984,000,000,000 compositions with the tier and without it, to
the path, because routing has already split the land it routes with.

A list is still read in a run that observes no land drop at all — no mana
question and no live effect — and there it costs groups and moves no number,
because nothing in that run can tell which land you played. Declare one where
something reads it.

### Mana, as a budget

`can_cast = "{U}"` being true does not mean you can do it six times. An opening
hand of one Island and six Opt casts **one** Opt on turn 1: the first one spends
the Island and the other five are dead cards. A model that fires whenever you
hold the card reports six filters and six draws, which overstates the turn by a
factor of six and looks entirely reasonable in a report. That is HANDS.md hand
1, and it is the hand this half exists for.

**Say which spells you would cast.** The pool is contested — one Island, one Opt
and one Preordain is one spell cast and two left in hand — so the priority is
declared in the file, exactly as the land drop is:

```toml
[casting]
prefer = ['name:"Trinket Mage"', 'name:"Lantern of Insight"']
```

**Then count what it paid for**, with `cast`:

```toml
[[criterion]]
name = "Trinket Mage cast by turn 3"
require = [{ turn = 3, cast = 'name:"Trinket Mage"', min = 1 }]

[[expect]]
name = "Opts cast by turn 5"
turn = 5
cast = 'name:"Opt"'
```

Worked on the hand it exists for — three seven-card decklists that differ by one
card, one criteria file, and the opening hand is the whole library so every
answer is a yes or a no:

```
$ gauntlet test hand-1.txt six-opts.criteria.toml    # Island, Opt x6
     an Opt cast on turn 1         100.00%
     two Opts cast on turn 1         0.00%
     six Opts in the opening hand  100.00%
     Opts cast by turn 1           mean 1.00
                                   1: 100.0%
```

Swap the Island for an Undercity Sewers — a tapland — and *an Opt cast on turn 1*
is **0.00%** while the last row is still 100%. Play seven Opts and no land and it
is 0.00% on every turn there is. The third row is the number a model that fired
on the holding would have reported as castings: the cards really are all there,
and holding them is what casting them is not.

The list is read in order and the **first entry the pool can still pay for is
cast**, then the next, until the mana runs out. The bill is **added up and
settled once** rather than asked spell by spell: casting a `{1}{W}` and a
`{1}{U}` out of two lands is one payment of four sources, which two independent
`can_cast` answers would both have called payable. A spell that is cast
**leaves the hand**, so the same copy cannot be cast twice and the count of what
you are still holding goes down. A **tie** inside one entry goes to the cheaper
cost — inside one entry you said you wanted them equally, so the only sense in
which one is better is that paying for it leaves more of the pool — and then to
the card your decklist names first.

**A spell the list does not name is not cast**, and that is the one place this
differs from `[land_drop]`, where a land nobody ranked is still played. The
reason is width: "any other land" costs one query bit, and "any other spell"
would make every card in your deck carry its own mana cost into the grouping,
which splits a Commander library forty ways along a line nobody asked about. So
the list is **the line you are asking about** rather than a preference over your
whole deck, which is the same reading the gate already takes of the land drop —
nobody plays their lands badly, and nobody casts the spell you did not ask
about.

**A gate beside a budget asks what the line left.** In a file that declares
`[casting]`, `can_cast` is answered against what the declared line did *not*
spend. One pool, one accounting: answering it against the whole turn's lands
while a declared line had already taken them would be two claimants on one
resource. A file that declares no casting priority spends nothing, so nothing
moves.

**Every run that cast by policy says so**, beside the land drop and the
tapped-ness assumptions, and it carries one claim the others do not:

```
note: the spells cast here are decided by the priority this file declared, and every
      number below that depends on what was cast depends on it:
      1. "name:\"Opt\""
      Then a spell this list does not name is not cast at all. Ties: a tie inside one
      entry goes to the cheaper cost, then to the card this decklist names first.
```

The same list is in the JSON as `casting`. A file that declares none has no
`casting` field at all, which is the other fact: that run cast nothing.

**The commander is cast from the command zone**, and asking about it takes no
new key. An entry that names your commander puts it in the line, and `cast`
counts it like any other spell:

```toml
[casting]
prefer = ['name:"Rashmi and Ragavan"']

[[criterion]]
name = "commander cast by turn 4"
require = [{ turn = 4, cast = 'name:"Rashmi and Ragavan"', min = 1 }]
```

A commander is **never drawn** — it is not in the library, so no deal puts it
in hand and no count of the library or the hand finds it — and it is **always
there** to be cast, from turn 1. It is paid for out of the same pool as
everything else the line casts, in the order the list gives, so a line
`[commander, Opt]` on four lands spends all four on the commander and casts no
Opt that turn. It is cast **once**: casting it takes it out of the command zone
and nothing puts it back, so commander tax never comes up. A permanent the line
cast is counted on the battlefield, and so is the commander; a copy the line
has not cast is in hand and not in play. At equal cost
inside one entry a card from the library is cast before it. A commander the
list does not name is not cast, which is every file written before this: none
of their numbers move. The run's note names the commanders the line cast from
the command zone, and so does the JSON, as `casting.from_command_zone`.

HANDS.md hand 37 is the worked case — twelve lands of the right colours cast
`{1}{G}{U}{R}` on turn 4 and not turn 3, and twelve Forests put four lands in
play on turn 4 and never cast it. On the decks, from
`decks/loam-commander.criteria.toml` and, for Rashmi, the file that asked it
until the Lantern north star folded it in (`lantern-commander.criteria.toml`,
last run at e1a49e4; [#100](https://github.com/cramt/progress-engine/issues/100)):

| Commander cast by | Rashmi and Ragavan (`lantern.deck.toml`), play / draw | Borborygmos and Fblthp (`loam.deck.toml`), play / draw |
|---|---|---|
| turn 4 | 42.50% / 51.49% | — (five mana) |
| turn 5 | 53.01% / 61.30% | 51.99% / 62.10% |
| turn 6 | 61.49% / 68.68% | 62.16% / 70.85% |
| turn 7 | | 70.84% / 78.04% |

All **sampled**, ± 0.11 each, and that is width rather than choice: a
three-colour cost keeps every land's palette over `{G}{U}{R}` and whether it
enters tapped, which is fifteen land profiles on both decks and sixteen groups
— 698,548,224 compositions at turn 4 on the play, against a ceiling of five
million. They are floors, for the reasons every mana number here is: the line
names no mana rock, so none is cast and only lands pay (the rocks are the next
paragraph but one), Rashmi's Treasure does not pay, and a land whose
tapped-ness is the pilot's choice is assumed tapped.

**Why those questions are files of their own.** One pool is one accounting: a
`can_cast` beside a line asks what the line left. Declared in the old
`decks/lantern.criteria.toml`, a union of gates, the commander line would have
repriced every route in it as *after the commander was paid for* — route 1's
control, `{1}` payable by turn 5, read 93.4% beside it and 99.6% without. The
north star is both halves at once, and that wants a line naming both: for
Lantern that is `decks/lantern.criteria.toml`
([the Lantern north star, as one line](#the-lantern-north-star-as-one-line)),
and for Loam `decks/loam.criteria.toml`
([The Loam north star](#the-loam-north-star-one-line)), where the commander is
cast by turn 5 on 52.11% / 59.10% of games, dorks and all, beside everything
else the line spends its lands on. (The Loam column above was re-read from
`docs/baseline` for #101; it had not been updated since #82.)

**A rock or a dork is mana once the line has cast it**
([ADR-0018](docs/adr/0018-rocks-and-dorks-are-sources-the-line-casts.md)).
Name it in `[casting] prefer` like any spell, and once it resolves it adds what
the effect library says it adds (`adds`, [Effects](#effects)) of the colours its
card makes — Sol Ring `{C}{C}`, a Talisman `{C}` or one of its two colours,
Arcane Signet one of your commander's colours, Birds of Paradise any colour. A
rock the line does not name is never cast, so it never makes mana, and every
file written before this names none: none of their numbers moved. Three rules
decide when that mana pays:

- **A rock pays for what the line casts after it, the turn it is cast, and
  never for itself.** Its own cost is paid while it is still a spell. So a
  turn's bill is no longer one matching: Island, Sol Ring and Memory Lapse is
  `{1}` then `{1}{U}` against an Island and `{C}{C}`, and a single matching over
  the sum lets the Island take the `{U}` and Sol Ring pay for itself. The bill
  is settled in stages instead, each spell against the sources already there
  when it was cast (HANDS.md hand 27), and the lands a turn can tap are still
  its land drops, however much a rock adds on top.
- **A dork is summoning-sick**, so it adds from the turn after it is cast
  (hand 31). A rock that enters tapped is declared with `after = 1` on its
  effect, and waits the same way.
- **The line is read again from the top once a rock grows the pool**, so
  `[Mind Stone, Sol Ring]` off one Island casts both on turn 1 (hand 28).

A card the line casts that could make mana in some game and is counted as making
none is named: Fellwar Stone, whose mana is what an opponent's land could make
and there is no opponent (CR 106.7), and Lotus Cobra, whose mana is a landfall
trigger. Each makes every number that casts it a lower bound. A spell with
improvise, affinity or convoke pays its printed cost, or the one its effect
declares, in full, and is named too. The
run's note says all of it beneath the line:

```
      Mana sources once cast — a rock's mana pays only for what the line casts after it, and a dork's from the next turn:
      Arcane Signet: adds 1 of {U}{R}{G}
      Talisman of Creativity: adds 1 of {U}{R}{C}
```

and the JSON carries it as `casting.sources` (card, `adds`, `makes`, `waits`),
`casting.uncounted` and `casting.printed_cost`.

On the decks, from `decks/lantern-rocks.criteria.toml`,
`decks/lantern-rocks-first.criteria.toml` and `decks/loam-rocks.criteria.toml`,
beside the lands-only answers the commander files give (Rashmi's from the
folded `lantern-commander.criteria.toml`, as above):

| Commander cast by, play / draw | lands only | the commander, then the rocks | the rocks, then the commander |
|---|---|---|---|
| Rashmi and Ragavan, turn 4 | 42.50% / 51.49% | 55.16% / 64.74% | |
| Rashmi and Ragavan, turn 5 | 53.01% / 61.30% | 65.79% / 73.69% | 65.46% / 73.43% |
| Borborygmos and Fblthp, turn 4 | 0.00% / 0.00% | 9.56% / 12.40% (dorks) | |
| Borborygmos and Fblthp, turn 5 | 51.99% / 62.10% | 56.52% / 66.37% (dorks) | |

All sampled, ± 0.11 or less. The Lantern line names Sol Ring, Arcane Signet, the
three Talismans and Mind Stone, and costs 24 groups where the lands-only line
costs 18 — 675,429,580,800 compositions at turn 5 on the play; the Loam line
names Birds of Paradise and Elvish Mystic, 18 groups against 16. Each file
answers in about half a second. Each is its own file because naming a rock in
the commander files' line would have moved their numbers from *lands only* to
*with rocks*, which is this question rather than theirs; and listing the rocks
first costs games the commander-first line wins, because a rock the pool can pay
for is cast before the commander is asked about. `checker/` asks all of them
from the rules, with its own line model, and agrees on both seats.

**What it does not model is the draw.** Opt is *scry 1, draw 1*, and only the
casting is counted — a `look` on an `on = "cast"` effect is still refused by
name, though a `fetch` on one is not: see
[Tutors](#tutors-and-a-library-that-shrinks).
That is a measurement rather than a shrug. Every card the walk might or might
not draw needs a checkpoint of its own, because an unordered pair cannot say
which of two revealed cards the draw took; each checkpoint multiplies the
enumeration by the group count; and a turn with *T* mana can cast *T* cantrips,
so one extra checkpoint per turn is the **floor**. Measured on the classes in
the table below:

| Line | groups | exact to, today | exact to, with one replacement draw a turn |
|---|---|---|---|
| a `{1}` one-drop | 4 | turn 8 — turn 9 is 7,864,320 | turn **4** (turn 5 would be 31,457,280) |
| `{1}{G}{G}` | 6 | turn 5 | turn **2** (turn 3 would be 6,158,592) |
| a two-spell line with a colour | 7 | turn 5 | turn **2** (turn 3 would be 28,840,812) |

Both north stars ask about turn 5, so a replacement draw would be sampled on
every question this tool exists to answer. That is a percentage changing kind
rather than a feature, so it is refused and the refusal carries the numbers:
[#57](https://github.com/cramt/progress-engine/issues/57).

**Tutoring is**, and it is the next section. A cast spell can go and get a named
card out of the library, which is a deterministic removal rather than a draw —
see [Tutors, and a library that shrinks](#tutors-and-a-library-that-shrinks).

**What it costs.** More than the gate, and the reason is worth stating: a cast
spell leaves the hand, so a criterion counting `cat:"Ramp"` beside a budget
depends on what the pool paid for three turns earlier — which depends on the
manabase and on what each named spell costs. So **a file that declares
`[casting]` prices the manabase on every question in it** and reads every turn
rather than a total. A file that declares none pays nothing.

Measured on the decks in `decks/`, against an index synced with oracle tags:

| Deck and line | Turn | groups / compositions | |
|---|---|---|---|
| `lantern.deck.toml`, `cast` Lantern of Insight (`{1}`) | 4 | 4 / 7,680 | **exact**, 0.16s |
| | 5 | 4 / 30,720 | **exact**, 0.18s |
| | 6 | 4 / 122,880 | **exact**, 0.25s |
| | 7 | 4 / 491,520 | **exact**, 0.52s |
| | 8 | 4 / 1,966,080 | **exact** |
| | 9 | 4 / 7,864,320 | sampled — 57% over |
| `lantern.deck.toml`, `cast` Trinket Mage (`{2}{U}`) then Lantern | 3 | 7 / 84,084 | **exact**, 0.26s |
| | 4 | 7 / 588,588 | **exact**, 0.84s |
| | 5 | 7 / 4,120,116 | **exact**, 4.4s |
| | 6 | 7 / 28,840,812 | sampled |
| `loam.deck.toml`, `cast` Life from the Loam (`{1}{G}{G}`) | 4 | 6 / 171,072 | **exact**, 0.44s |
| | 5 | 6 / 1,026,432 | **exact**, 1.9s |
| | 6 | 6 / 6,158,592 | sampled — 23% over |

**The cheapest budget question is wider-reaching than the cheapest gate
question**, which was not the expected result. `can_cast = "{1}{U}"` names a
colour, so it keeps four land groups and goes over the ceiling at turn 7; `cast`
on a card that costs `{1}` demands no pip at all, so the manabase is two groups
— tapped and untapped — and four groups stay exact through turn 8. Asking what
you cast can be *cheaper* than asking what you could have paid, because the
colour comes from the card rather than from the question.

A colour puts it back where the gate is: `{1}{G}{G}` is six groups and exact
through turn 5, and a two-spell line with a colour in it is seven and exact
through turn 5. The ceiling is unchanged for the gate — `{1}{U}` on either deck
is still 5 groups, 1,031,250 compositions and exact at turn 6, and still 3% over
at turn 7.

Reproduce any row with the `enumerations` block:

```
$ gauntlet sync --index /tmp/index.jsonl
$ gauntlet test decks/lantern.deck.toml /tmp/route-b.toml --index /tmp/index.jsonl \
    | jq -c '.enumerations[]'
{"criteria":["Trinket Mage cast by turn 3"],"expectations":[],
 "queries":["name:\"Trinket Mage\"","name:\"Lantern of Insight\""],"turns":[3],
 "reading":"per-turn","pips":["{U}"],"groups":7,"compositions":84084,"method":"exact"}
```

Note the `pips`: the criteria file names no colour anywhere, and the enumeration
still tells a blue source from every other land, because the card data says
Trinket Mage costs `{2}{U}`. That is a grouping decision nothing in the file
states, which is exactly the kind this block exists to report.

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
| `on` | when it fires: `landdrop`, `cast`, `activate`, `attack` or `landfall`, see below, [An activation the line pays for](#an-activation-the-line-pays-for-expedition-map) and [Attack and landfall](#attack-and-landfall-a-mill-that-fires-again) |
| `cost` | what the line pays: on a cast, in place of the printed cost; on an activation, to activate a copy in play. See [A cost the line pays](#a-cost-the-line-pays-that-is-not-printed-dizzy-spell-and-whir-of-invention) |
| `sacrifice` | beside `after`, the card that waited leaves play when the effect resolves (a Saga); on an activation, paying it sacrifices the card (Expedition Map) |
| `to_graveyard` | the routing policy: which examined cards go to the yard. `"*"` is all of them, which is mill. Absent means none of them |
| `fetch` | the cards it goes and gets out of the library, highest priority first. See [Tutors](#tutors-and-a-library-that-shrinks) |
| `to` | where a fetched card is put: `hand` or `battlefield` |
| `adds` | how much mana a card adds a turn once the `[casting]` line has cast it, `on = "cast"` only, of the colours its card makes. With `after = n` it adds nothing for `n` turns, which is a rock that enters tapped. See [Mana, as a budget](#mana-as-a-budget) and [ADR-0018](docs/adr/0018-rocks-and-dorks-are-sources-the-line-casts.md) |
| `mill` | how many cards a cast, an attack or a landfall puts off the top of the library into the graveyard. Not on a `landdrop`, whose way to do that is a `look`. See [Mills](#mills-a-spell-that-turns-cards-over) |
| `keep`, `keep_only` | how many of a mill's cards the card lets go to hand instead, and which cards it allows |
| `keep_every` | the cards of a mill the card puts in hand whatever you want: Wrenn and Seven's lands |
| `to_hand` | your choice among what `keep` allows, highest priority first. Absent keeps nothing |
| `returns` | after a mill, every land card in the graveyard matching this goes onto the battlefield tapped, whatever you want: Lumra's lands |

**Looking is a land drop, fetching can be a cast.** Playing a land is free and
hard-capped at one a turn, so by turn *T* at most *T* of those have happened
whatever your deck — which is what keeps a `look` bounded and exact. `on =
"cast"` fires too, because the budget knows which spells a turn paid for
([Mana, as a budget](#mana-as-a-budget)) — but a **`look` on a cast is refused
by name**, and that has not changed. A replacement draw makes how many cards you
have seen by turn *T* depend on the path rather than on the schedule, which is
one extra enumeration checkpoint per turn at the floor and puts every question
this tool exists for over the ceiling
([#57](https://github.com/cramt/progress-engine/issues/57)). A `fetch` on a cast
is a subtraction and costs nothing. A `mill` on a cast turns cards over and
takes every one of them off the top at once, so it is one block dealt only on
the paths that cast it ([Mills](#mills-a-spell-that-turns-cards-over)).

**The library never says where cards go** — unless the card does. Every
shipped entry that looks declares `match`, `look` and `on`, and no entry
declares `to_graveyard` or `to_hand`. That split is the whole design. The one
destination it states is a mill's graveyard, which the card compels rather
than you choosing it ([ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md)). The same Undercity Sewers wants Life from the Loam in the graveyard in
one deck and on top of the library in another — so the destination is part of
*your question*, not a property of the card, and the tool guessing at it would
be answering something nobody asked. An effect with no destination leaves every
card it looks at exactly where it was, which is where the next draw was going to
find it anyway, so it moves no number at all.

Write `to_graveyard` yourself and the same surveil starts binning:

```
$ gauntlet test loam.deck.toml loam.criteria.toml
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

And two that look at nothing, `on = "cast"`: the amount of mana a rock or dork
adds once the line casts it, which Scryfall's `produced_mana` leaves out
([ADR-0018](docs/adr/0018-rocks-and-dorks-are-sources-the-line-casts.md)). Both
require `otag:mana-rock or otag:mana-dork` and exclude any card whose mana is
conditional, delayed, restricted, variable, or paid for with more than a tap, and
any double-faced card, because an entry may understate a card and must never
overstate one. The full queries are in
`crates/ichormoon-gauntlet/toml/src/standard-effects.toml`.

| Reads | `adds` | Scryfall, 2026-09-26 | In `decks/` |
|---|---|---|---|
| `o:"{T}: Add"` | 1 | 364 cards | Mind Stone, Arcane Signet, three Talismans, Birds of Paradise, Elvish Mystic |
| `o:"{T}: Add {C}{C}."`, declared after, so last-wins | 2 | 15 cards | Sol Ring |

Fellwar Stone (with no opponent it makes nothing) and Lotus Cobra (landfall, not
a tap) are deliberately not sources.

And four mills, `on = "cast"`, keyed by name because the count is printed on
each card and no tag carries it: Aftermath Analyst (`mill = 3`), Malevolent
Rumble and Midnight Tilling (`mill = 4`, `keep = 1`, `keep_only =
"is:permanent"`) and Wrenn and Seven's +1 (`mill = 4`, `keep_every = "t:land"`).
And three whose mills are triggers: Six (`on = "attack"`, `mill = 3`, `keep =
1`, `keep_only = "t:land"`), Icetill Explorer (`on = "landfall"`, `mill = 1`)
and Lumra, Bellow of the Woods (`mill = 4`, `returns = "t:land"`).
Each fires only where the `[casting]` line names its card, so every other file
is answered exactly as it was.

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

### Tutors, and a library that shrinks

Trinket Mage does not draw you a Lantern of Insight. It takes the Lantern **out
of the library** and puts it in your hand, and everything after that is against
a smaller deck with a different composition. For a long time this engine could
not say so: the enumeration took the group sizes once and computed what remained
as `groups - drawn`, so the population was a constant of the whole path by
construction.

A tutor is a **deterministic removal from a named group**. Given what the path
has done so far there is one answer, so it costs no branch — it is a subtraction
from the pool the next draw comes out of, not a second distribution laid over
it. Which means it is free: Route B on `decks/lantern.deck.toml` is seven groups and
4,120,116 compositions at turn 5 with the fetch and without it, to the
composition.

**Say what it fetches:**

```toml
[[effect]]
match = 'name:"Trinket Mage"'
on = "cast"
fetch = ['name:"Lantern of Insight"']
to = "hand"
```

`fetch` is a **declared priority over queries**, read in order, and the first
entry the library still holds is the one it takes — the same mechanism as
`[land_drop]`, `[casting]` and selection routing, over a fourth resource rather
than a fifth policy language. A tie inside one entry goes to the card your
decklist names first. A tutor that finds none of them fetches nothing, which is
what a tutor does when the card is already gone.

**Every run that fetched says what it fetched**, on stderr and as `fetch` and
`to` in the JSON, beside the land drop and the casting line. A number that hinged
on a declared policy and did not name it is the bug this project exists to
prevent.

```
note: effect "name:\"Trinket Mage\"" (on cast)
      applies to 1 card: Trinket Mage
      and fetches, to your hand, the first of these the library still holds:
      1. "name:\"Lantern of Insight\""
      Ties: the card this decklist names first. A tutor that finds none of them fetches nothing.
```

**What it is worth, on the deck it was built for.** `decks/lantern-route-b.criteria.toml`
is the Lantern north star's second route on its own. Delete the `[[effect]]`
block and re-run:

| On `decks/lantern.deck.toml`, on the play | drawn | fetched |
|---|---|---|
| Trinket Mage cast by turn 3 | **4.99%** | **4.99%** |
| Trinket Mage and a Lantern both cast by turn 5 | 0.78% ± 0.02 | **7.67% ± 0.06** |
| Lanterns still in the library on turn 5 (mean) | 0.8889 | **0.8114** |

The first row does not move and must not: what a spell does when it resolves
cannot change whether the pool paid for it. The second row is the route, and
0.78% was the deck drawing both halves naturally.

**A card a tutor puts in hand is one the line can cast that turn**, wherever the
line lists it: after the fetch the line is read again from its top, as it is
after a spell that draws. `decks/loam.criteria.toml` is the case that
needs it — its line reads Loam before Spellseeker, so that a Loam
already in hand is not held a turn behind a three-mana tutor with nothing to
find, and five lands cast Spellseeker and then the Loam it fetched:

| On `decks/loam.deck.toml`, Loam in the graveyard by turn 5 | play | draw |
|---|---|---|
| casting Loam you drew | 9.92% | 11.23% ± 0.07 |
| ... and the Loam Spellseeker fetched | **17.08% ± 0.08** | **19.58% ± 0.09** |

With the fetch deleted and Spellseeker still in the line it reads 9.95% ± 0.07
and 11.22% ± 0.07, so casting Spellseeker moves nothing and the whole
difference is the card it went and got (HANDS.md hand 36).

**A fetchland is not a filter, and the distinction matters.** Scry and surveil
examine N cards off the top; a fetchland removes a card from the library and
shuffles, then is not there any more. Modelling it as "look at N" would produce
numbers that are wrong and plausible. So it is a tutor with `to = "battlefield"`,
which puts the land it found down **in place of** the fetchland that went and
got it:

```toml
[[effect]]
match = "otag:fetchland"
on = "landdrop"
fetch = ["t:land"]
to = "battlefield"

[land_drop]
prefer = ["otag:fetchland", "t:land"]
```

`otag:fetchland` is 54 cards, against the ten-card `is:fetchland` cycle, and the
index fetches it at sync time. The `[land_drop]` table is **required** beside a
land-drop fetch rather than optional: the fetch happens on the drop and replaces
the land that made it, so a run that cannot say which land you played cannot say
what you fetched either.

**Does deck thinning improve your subsequent draws?** Everyone has an opinion and
almost nobody has the number. `decks/loam-thinning.criteria.toml` is the number,
exactly, for a deck where fetchlands are a large part of why it functions:

| On `decks/loam.deck.toml`, on the play | as a land | fetching | moved by |
|---|---|---|---|
| some Loam Access by turn 5 | 80.9583% | 81.0158% | +0.0575 pp |
| some Loam Access by turn 8 | 88.3546% | 88.4310% | +0.0764 pp |
| two Loam Access by turn 8 | 58.9176% | 59.0695% | +0.1519 pp |
| lands drawn by turn 8 (mean) | 6.2857 | 6.2680 | −0.0177 |
| fetchlands in play by turn 8 (mean) | 0.5714 | 0.0000 | |

**Yes, exactly, and negligibly.** The largest of those is one game in 658. The
bottom row is why: four fetchlands in 98 cards is 0.57 of one cracked by turn 8
on average, each removing one land from a library of about ninety. And the row
above it is the honest other half, which goes the *wrong* way — the land the
fetch found is already in play, so it is one fewer land left to draw. Thinning
trades a little of your land count for a little spell density. It is real, it is
now exact, and it is not why you play fetchlands.

**What a fetched land taps for is refused by name.** A Scalding Tarn fetches
untapped and a Terramorphic Expanse fetches tapped, `otag:fetchland` holds both,
and nothing on the land it *found* tells them apart — so a `can_cast` or a
`cast` clause beside a battlefield fetch is refused rather than answered in
whichever direction happens to flatter. What left the library is exact; what it
makes is not modelled. A land that `on = "cast"` (or `"activate"`) with `to =
"battlefield"` puts down — Rampant Growth, Nature's Lore, Cultivate, Wood Elves —
is counted on the battlefield at once and pays from the next turn, read as
entering tapped because whether it does is a fact about the spell, and no tag
separates Rampant Growth from Nature's Lore. That is exact for Rampant Growth
and a floor for Nature's Lore, the run says so beside the fetch, and it needs
`[land_drop]` declared, because only a declared drop records which lands are
standing ([ADR-0025](docs/adr/0025-a-land-a-spell-puts-down-is-tapped-and-pays-from-the-next-turn.md),
[HANDS.md](HANDS.md) hand 62):

```toml
[land_drop]
prefer = ['t:land']

[casting]
prefer = ['name:"Llanowar Elves"', '''name:"Nature's Lore"''', 'name:"Rampant Growth"']

[[effect]]
match = '''name:"Nature's Lore" or name:"Rampant Growth"'''
on = "cast"
fetch = ['t:forest']
to = "battlefield"
```

Anything else a cast may put there; see the next section.

**What it costs: no width, and about five per cent of the wall clock.** A
removal is decided once per path prefix rather than branched over, so the
enumeration is the one that was already there. Measured on `decks/lantern.deck.toml`,
Route B's two-spell line, on the play:

| Turn | groups / compositions | drawn | fetching |
|---|---|---|---|
| 3 | 7 / 84,084 | **exact**, 0.27s | **exact**, 0.27s |
| 4 | 7 / 588,588 | **exact**, 0.88s | **exact**, 0.96s |
| 5 | 7 / 4,120,116 | **exact**, 4.9s | **exact**, 5.1s |
| 6 | 7 / 28,840,812 | sampled | sampled |

**The ceiling falls exactly where it did**: exact through turn 5, over at turn 6
by a factor of six, on both. The five per cent is a replay of the walk at each
checkpoint of each prefix, which is how the run learns what the path has fetched
without a second reading of the tutor's list, and a run that declares no tutor
does not do it at all.

**A land-drop fetch does cost width**, and for a reason that is not the removal:
it is a live effect, and a live effect stops a class from collapsing its
checkpoints into one total, because routing reads the order cards came off the
top. `decks/loam-thinning.criteria.toml` asks *some Loam Access by turn 8* on 2
groups and 15 compositions with no effect declared and on 4 groups and 1,966,080
with one — 0.3s against 6.6s, both exact. That is the per-turn reading, and it
would be the same price for a surveil land.

**What is not here is exiling off the top.** Devourer of Destiny exiles three
cards, and those are a *random sample* — so what remains is a distribution
rather than a subtraction, and it branches the path the way a draw does. That is
the expensive half of
[#18](https://github.com/cramt/progress-engine/issues/18) and it is filed rather
than approximated.

### A cast that puts a card onto the battlefield: Tezzeret the Seeker

Tezzeret the Seeker is `{3}{U}{U}` and enters with four loyalty, and his −X
searches the library for an artifact with mana value X or less and puts it
**onto the battlefield**. The Lantern is mana value 1, so −1 finds it, at
sorcery speed, the turn he resolves. It is written as a cast fetch with `to =
"battlefield"`, reading the −1 as part of his cast
([ADR-0019](docs/adr/0019-a-tutor-route-is-something-the-line-pays-for.md)):

```toml
[[effect]]
match = 'name:"Tezzeret the Seeker"'
on = "cast"
fetch = ['name:"Lantern of Insight"']
to = "battlefield"

[casting]
prefer = ['name:"Lantern of Insight"', 'name:"Tezzeret the Seeker"']
```

**What it moves.** The Lantern leaves the library and is on the battlefield
from the turn the Seeker is cast. It never touches the hand and is never cast,
so a `cast` clause about it does not count it and a `zone = "battlefield"`
question does — which is why a battlefield question about a card only the
Seeker can put there is answered, as one about a Saga's chapter is. Nothing
else moves: what he does when he resolves cannot change whether the pool paid
for him. That is the whole difference from Trinket Mage, whose Lantern still
costs `{1}` after it arrives. HANDS.md hand 40:

| Ten Islands, the Seeker and the Lantern, on the play | no fetch | fetch to battlefield |
|---|---|---|
| Tezzeret the Seeker cast by turn 5 | 37/44 = 84.09% | 37/44 = 84.09% |
| Lantern on the battlefield by turn 5 | 11/12 = 91.67% | **100%** |
| Lantern cast by turn 5 | 11/12 = 91.67% | 11/12 = 91.67% |
| Lantern still in the library on turn 5 | 1/12 = 8.33% | **0%** |

**What it will not put there is a land, or anything that is not a
permanent.** The refusal is on what the priority can match in *this* deck: a
cast fetch onto the battlefield whose priority matches a land is Rampant
Growth, and whether that land enters tapped is a fact about the spell that no
tag carries; an instant or a sorcery cannot be put onto the battlefield at all.
The same shape as the Saga's rule below, which is the mirror of a fetchland's.

**On the deck it was built for.** `decks/lantern-route-seeker.criteria.toml` is
the route on its own, beside the Lantern cast from hand. Delete the
`[[effect]]` block and re-run:

| On `decks/lantern.deck.toml` | play, drawn | play, fetched | draw, drawn | draw, fetched |
|---|---|---|---|---|
| Tezzeret the Seeker cast by turn 5 | 2.80% | 2.84% | 4.30% | 4.22% |
| Lantern of Insight cast by turn 5 | 10.99% | 11.10% | 12.11% | 12.14% |
| Lantern on the battlefield by turn 5 | 10.99% | **13.76%** | 12.11% | **16.03%** |

All sampled, ± 0.04 to 0.08: nine groups and 42,220,035 compositions at turn 5,
route B's width, in 0.8s. The first two rows move inside their error bars and
the third is the route, +2.8 points on the play and +3.9 on the draw. The line
casts the Seeker off lands alone, so these are floors until rocks join the
bill. Whir of Invention, the route's other card, is played at a declared
cost; see the next section.

### A cost the line pays that is not printed: Dizzy Spell and Whir of Invention

Some tutors are not played for their printed cost. Dizzy Spell is a `{U}`
instant that does nothing for a Lantern deck; its **transmute** — `{1}{U}{U}`
and discard it, at sorcery speed — finds a card with its mana value, 1, which
is the Lantern. Whir of Invention is `{X}{U}{U}{U}`, and the X is the pilot's.
An effect may say what the line pays for its card with `cost`, and the line
bills that instead of the printed cost, both in the turn's bill and in the pips
the question is enumerated on
([ADR-0019](docs/adr/0019-a-tutor-route-is-something-the-line-pays-for.md)):

```toml
[[effect]]
match = 'name:"Dizzy Spell"'
on = "cast"
cost = "{1}{U}{U}"
fetch = ['name:"Lantern of Insight"']
to = "hand"

[[effect]]
match = 'name:"Whir of Invention"'
on = "cast"
cost = "{1}{U}{U}{U}"          # X = 1
fetch = ['name:"Lantern of Insight"']
to = "battlefield"
```

`cost` is a value, like `look` and `after`, not a priority: which card the
line plays is still `[casting] prefer`. It must be a whole amount, so `{X}` and
hybrid are refused in it as `can_cast` refuses them, and it belongs on a cast (or
on an activation, below, where it is what activating the card costs).
A card whose printed cost holds `{X}` is refused in `[casting]` **unless** its
effect declares one; then the printed cost is never read. A `cast` clause counts
a transmutation as a casting of the card, and every run prints the declared
cost beside the printed one:

```
      Played at a cost an effect declares, not the printed one; a `cast` clause counts it as a casting:
      Dizzy Spell: billed {1}{U}{U}, printed {U} (effect "name:\"Dizzy Spell\"")
      Whir of Invention: billed {1}{U}{U}{U}, printed {X}{U}{U}{U} (effect "name:\"Whir of Invention\"")
      Cost reductions are not modelled, so these pay their printed or declared cost in full: Whir of Invention.
```

and the JSON carries the same under `casting.declared_costs`. Improvise is not
modelled, as no cost reducer is, so Whir pays its four whole and is a floor.

**What it moves.** Without `cost`, the same Dizzy Spell effect is billed `{U}`
and tutors on turn 1, which is the confident wrong number this exists to
prevent. HANDS.md hand 41, ten Islands, Dizzy Spell and the Lantern, on the
play:

| | printed `{U}` | declared `{1}{U}{U}` |
|---|---|---|
| Dizzy Spell played by turn 2 | 2/3 = 66.67% | **0%** |
| Lantern cast by turn 2 | 10/11 = 90.91% | **2/3 = 66.67%** |
| Lantern cast by turn 4 | 65/66 = 98.48% | 65/66 = 98.48% |

And hand 50, Whir at X = 1 in the same deck: never cast on turn 3, where an
`{X}` read as zero would cast it, and the Lantern on the battlefield by turn 4
on 130/132 deals.

**On the deck it was built for.** `decks/lantern-route-tutors.criteria.toml`
is `lantern-route-seeker.criteria.toml` with Dizzy Spell and Whir added to the
line, so the two files differ by those two cards:

| On `decks/lantern.deck.toml` | play, Seeker | play, + both | draw, Seeker | draw, + both |
|---|---|---|---|---|
| Lantern on the battlefield by turn 5 | 13.76% | **21.75%** | 16.03% | **25.87%** |
| Lantern of Insight cast by turn 5 | 11.10% | 16.60% | 12.14% | 18.86% |

Sampled, ± 0.04 to 0.10: eleven groups and 284,738,168 compositions at turn 5
on the play, about 1.2s a seat. +8.0 points on the play and +9.8 on the draw,
against ADR-0019's +5.35 and +6.38, which were measured after four other routes
were already in the line. Lands only, so floors until rocks join the bill.

### Mills: a spell that turns cards over

Aftermath Analyst mills three when it enters. Those are the three cards your
next draws would have found, so a mill is not a subtraction from a named group
the way a tutor is: it is a **random block** off the top, and what is left
after it is a distribution. It is dealt as one **sized gap**
([ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md)): the walk
asks, at each point of the path, how many cards the next gap deals, and a path
that never cast the Analyst deals nothing for it. The block is unordered
because every card in it is consumed at once.

**The card says where they go, and you say what it keeps.** The graveyard is
the one destination the standard library states, because the card compels it.
Which card Malevolent Rumble keeps is yours, and goes on the effect as
`to_hand`, a priority over queries like every other one:

```toml
[[effect]]
match = 'name:"Malevolent Rumble"'
on = "cast"
mill = 4
keep = 1
keep_only = "is:permanent"
to_hand = ['name:"Spellseeker"', 't:land']
```

`mill`, `keep` and `keep_only` are the card's, and repeated here because
last-wins overrides a whole entry. A Rumble with no `to_hand` keeps nothing and
bins all four, because that is what the card does when you choose nothing. A
list naming Life from the Loam cannot keep it: it is a sorcery, and the next
entry decides (HANDS.md hand 20). A card kept this way is in hand at once, so
the line may cast it that turn; a land kept this way waits for the next turn's
drop, even on a turn whose drop was not made. Every run that milled prints what
it kept and by which list:

```
note: effect "name:\"Malevolent Rumble\"" (mill 4, on cast)
      applies to 1 card: Malevolent Rumble
      puts what it mills in the graveyard, except up to 1 matching "is:permanent", kept in your hand by the first of these that holds one:
      1. "name:\"Spellseeker\""
      2. "t:land"
      Ties: the card this decklist names first.
```

**What it is worth to the Loam north star.** `decks/loam-cast.criteria.toml`,
whose line is now the north star's, added the four mills to it after
Spellseeker:

| On `decks/loam.deck.toml`, Loam in the graveyard by turn 5 | play | draw |
|---|---|---|
| casting the Loam, drawn or fetched by Spellseeker | 17.08% ± 0.08 | 19.58% ± 0.09 |
| ... and Rumble and Tilling | 18.46% ± 0.09 | 21.26% ± 0.09 |
| ... and the Analyst and Wrenn and Seven | **19.02% ± 0.09** | **21.82% ± 0.09** |

Both seats are sampled, as they were before: a class with a sized gap has no
closed-form width, so it is counted against the ceiling, and this one passes
it. `checker/` replays each mill from the card's text and agrees.

**A mill nothing reads is dealt last, and only as finely as the question reads
it** (ADR-0017 §4). A shuffled library does not care where in the order a
block of cards sits, so a mill that keeps none of its cards, in a run where no
tutor searches the library it came from, is dealt after the last turn instead
of where it fired, over only what the class's questions count in the
graveyard and the library — Loam or not Loam. Its cards are filed under the
turn it fired, so a clause about any turn reads what it would have. It is a
narrowing: it moves no number, and every committed file answers what it
answered before, to the digit. It changes what can be answered exactly.
`decks/loam-analyst.criteria.toml` is the Analyst's route alone:

| On `decks/loam.deck.toml`, turn 5, the Analyst then Loam | paths walked | answer |
|---|---|---|
| on the play, milled where it fired | 27,283,512 | 10.15% ± 0.07, sampled |
| on the play, dealt last | 1,459,454 | **10.19%, exact** |
| on the draw, dealt last | 8,468,217 | sampled |

Rumble and Tilling choose from their cards, Wrenn and Seven sends the lands
among them to hand, and `loam.criteria.toml` has Spellseeker searching the
library, so every mill there is still dealt where it fired. The sampler never
defers, which makes its agreement the test of the claim.

**Not here yet.** Vastlands Scavenger's Bind to Life mills seven, but it is a
copy cast later from a creature already in play, which is a second casting the
line does not make. And dredge, which would take the Loam back out, is
ADR-0017's last word.

### Attack and landfall: a mill that fires again

A spell mills once, the turn the line casts it. A creature that mills when it
attacks, or a permanent that mills when a land enters, mills again and again
for as long as it is on the battlefield. Each firing is the same thing a cast's
mill is: one block off the top, dealt where it fired, on the paths where it
fired ([ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md)
§1 names both). Two more words for `on`, and nothing else new: what either may
do is a `mill`, with `keep` and `to_hand` as a cast's, and anything else on it
is refused by name.

- **`on = "attack"`** fires once a turn for each copy the line cast on an
  earlier turn: a creature is summoning-sick the turn it arrives (CR 302.6),
  so Six cast on turn 3 mills on turns 4 and 5. Combat follows the main phase,
  so it fires after that turn's line, and a land it keeps waits for the next
  turn's drop. **The run assumes it attacks every turn and nobody blocks it or
  removes it**, because nobody else is at this table, and every run that fires
  one says so, as `ASSUMED:` in the report and `assumes` in the JSON.
- **`on = "landfall"`** fires once for each land that enters while its
  permanent is on the battlefield: the drop, the land a fetchland puts down in
  its place, and every land a spell returns. The drop comes before the line,
  so the Explorer cast on turn 4 sees turn 5's drop and not turn 4's.
- **`returns = "t:land"`**, beside a `mill`, puts every land in the graveyard
  onto the battlefield tapped once the mill is done: Lumra's four and whatever
  an earlier mill left there. Nobody chooses, so the library states it. A mill
  that returns lands is never dealt last, because what it returns is read off
  the graveyard. The lands took no drop, and a turn's bill is still held to its
  drops, so what they could pay for is a floor.

Icetill Explorer's other two lines, an additional land a turn and lands played
from the graveyard, are not modelled; its landfalls, and every number beside
them, are floors by that much. Six's retrace is a casting from the graveyard,
which no line makes. HANDS.md hands 58 to 60 pin each one.

**What they are worth to the Loam north star.** `decks/loam-cast.criteria.toml`
added all three to the end of its line, after the mills and the discards
([Discard](#discard-a-spell-that-draws-and-then-bins)), with Six keeping a
land:

| On `decks/loam.deck.toml`, Loam in the graveyard by turn 5, cast, milled or discarded | play | draw |
|---|---|---|
| the four mills and the three discards, land drop declared | 19.53% ± 0.09 | 22.14% ± 0.09 |
| ... and Six, Icetill Explorer and Lumra | **19.79% ± 0.09** | **22.57% ± 0.09** |

About a quarter of a point on the play and 0.4 on the draw. Six cast on
turn 3 mills six cards by turn 5; the Explorer, cast on turn 4 at the
earliest, one. Lumra costs six and the line casts no mana source, so it is
never cast by turn 5 and moves nothing here. The file's other two questions,
which count castings, move by less than two standard errors of the
difference, which is noise between two samples. `checker/` plays Six's attacks and the Explorer's landfalls from the cards'
text and the rules, and agrees.

### Discard: a spell that draws, and then bins

Frantic Search draws two and then makes you discard two. The draw is a sized
gap like a mill's, dealt the turn the line casts it; the discard is a zone move
from hand to graveyard, and **which cards go is yours**, declared once for the
whole file because the hand is one resource every outlet draws on
([ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md) §3):

```toml
[discard]
prefer = ['name:"Life from the Loam"', 't:land']
```

The standard library says what each card fixes, and never which cards:

| card | draws | discards | the card fixes |
|---|---|---|---|
| Frantic Search | 2 | 2 | and untaps three lands |
| Izzet Charm | 2 | 2 | its third mode, the one a line casts it for |
| Desperate Ravings | 2 | 1 | **at random** |
| Borborygmos and Fblthp, entering | 1 | any number | **land cards only** |

- **A forced discard walks the list.** Each entry gives up everything it holds
  until what is left to discard is less than that; that entry gives up the
  rest. After the last entry, the cards no entry names are one more entry.
- **A tie is priced, not broken.** Where an entry holds more than is left to
  take, or the unnamed cards do, every set of that many is as likely, and the
  exact engine walks every one of them at its chance — the reason the
  mulligan's `bottom` gives. Name more entries to make it smaller.
- **"Any number" is every eligible card the list names**, and nothing it
  does not. With no list it discards nothing.
- **At random ignores the list.** Desperate Ravings picks from the whole hand.
- **A discard in an activation's cost takes only what the list names**, and
  with too few of those in hand the card is not activated
  ([Artificer's Intuition](#an-activation-whose-cost-discards-artificers-intuition)).
- **A forced discard with no list is refused**, naming `[discard]` as the
  remedy (HANDS.md hand 21): which cards leave your hand is not the tool's to
  decide.
- **A discard that can take a land needs `[land_drop]`.** A land in play is
  not in your hand, and which lands are in play is which ones you played.
  Without a declared drop the mana reading assumes whichever lands pay, which
  names no land as still held, so the run is refused rather than guessed.
- **Frantic Search's untap is read as the lands that paid for it**, so the
  turn's mana is where it was before the spell. That is a floor: a pilot could
  untap three better lands.
- **A spell drawn mid-line is cast that turn** if the line reaches it and the
  pool still pays; **a land drawn mid-line waits** for the next turn's drop,
  even on a turn whose drop was not made (HANDS.md hand 24). Every run whose
  line draws says both.

Every run that discarded prints the list, beside the land drop and the line:

```
note: the cards discarded here are decided by the priority this file declared, and every
      number below that reads the hand or the graveyard depends on it:
      1. "name:\"Life from the Loam\""
      2. "t:land"
      Then a forced discard takes a card this list does not name only after every card it does, and an "any number" discard never takes one. Ties: a tie inside one entry, and among the cards no entry names, is settled at random, and every way it could fall is priced.
      And a card that discards at random ignores this list: every card in hand is as likely.
```

**What it is worth to the Loam north star.** `decks/loam-cast.criteria.toml`
added Frantic Search after the Loam, and Izzet Charm and Desperate Ravings after
the mills, with the list above. The discards made it declare its land drop —
taplands first, then any land in decklist order — which is a line the pilot
plays where the old reading was the best line, so it is worth less on its own:

| On `decks/loam.deck.toml`, Loam in the graveyard by turn 5 | play | draw |
|---|---|---|
| casting, Spellseeker and the four mills, no land drop declared | 19.02% ± 0.09 | 21.82% ± 0.09 |
| ... with the land drop declared | 18.43% ± 0.09 | 21.14% ± 0.09 |
| ... and the three discards | **19.53% ± 0.09** | **22.14% ± 0.09** |

So the discards are worth 1.1 points on the play and 1.0 on the draw, which is
ADR-0017's estimate. `checker/` plays the same declared line, discards and
land drop and all, and agrees.

**Not here yet.** Three Steps Ahead's draw-two-discard-one is a Spree mode that
costs {2} more than the card's printed {U}, and nothing yet says a cast costs
more than printed. Cavalier of Flame discards and then draws that many, which
is a discard before a draw whose size the discard decides. Flashback on
Desperate Ravings is a second casting from the graveyard. Borborygmos and
Fblthp's attack trigger is a later turn's.

### The Loam north star: one line

`decks/loam.criteria.toml` is the north star as the owner states it — Life
from the Loam put into the graveyard **and** Borborygmos and Fblthp cast, by
turn 5, by any line the pilot's own cards allow, on one budget — asked as one
criterion of two clauses against the same game. Its `[casting]` line casts
every route above: the commander, then the Loam, then Birds of Paradise and
Elvish Mystic, then Frantic Search, Spellseeker, the mills, the other two
discards and the three triggers. Its land drop plays the lands that enter
tapped first, then the fetchlands, then the colourless utility lands, then the
other nonbasics, basics last; its discard list bins the Loam and then a
nonland card the line never casts, and names no land, so Borborygmos keeps
every land it could have binned.

It asks the north star at turns 4 to 7 and each half beside it, from the same
games:

| On `decks/loam.deck.toml`, play / draw | turn 4 | turn 5 | turn 6 | turn 7 |
|---|---|---|---|---|
| **Loam put into the graveyard and the commander cast** | 0.53% / 0.80% | **8.22% / 10.71%** | 17.93% / 22.06% | 23.98% / 27.93% |
| Loam put into the graveyard | 14.14% / 16.06% | 17.44% / 19.55% | 24.13% / 27.02% | 28.64% / 31.51% |
| Borborygmos and Fblthp cast | 7.24% / 8.98% | 52.11% / 59.10% | 72.05% / 79.14% | 80.78% / 86.22% |

Sampled, 200,000 hands, ± 0.11 or less: each turn is a class of 44 groups
whose width passes the ceiling. No criterion carries a bound; the owner
chooses the turn and the share from the curve. The order, the land drop and
the list were each chosen by measuring the alternatives, which the file
records: at turn 5, putting the commander last in the line costs 2.5 points
on the play and 3.7 on the draw, a land drop of taplands and then decklist
order 0.7 and 0.9, and a discard list naming lands second 0.3 and 0.4.

Lumra's return is declared away in that file: the lands it returns pay for
nothing here, and with the return in the line the bound on what the library
can lose (`LibraryRunsOut`) counts every land in the deck coming back and
refuses turn 6 and turn 7. At turn 5 it is worth about 0.1 points on the play
and less than its ± on the draw, which is the floor this leaves.

Every run whose line can reach the graveyard names the cards with dredge it
never dredged, because the engine never takes a dredge (ADR-0017) and that is
a line the pilot could play rather than the best one:

```
note: this run never dredges. Life from the Loam, Shenanigans have dredge, and this line can put cards in the
      graveyard, but no draw here is ever replaced by a dredge (ADR-0017). Dredge is a
      may, so that is a line the pilot could play: every number below is that line's,
      a floor under the best line, which could dredge where dredging helps.
```

The JSON carries the same list as `never_dredged`. A run takes about three
minutes a seat on four cores, most of it counting each turn's paths against
the ceiling before sampling them. `checker/` plays the same line from the
cards' text and the rules, at every turn and on both seats, and agrees.

### Delayed effects: Urza's Saga

Urza's Saga is an Enchantment Land. It arrives on a land drop with a lore
counter, gains one after each of your next two draw steps, and chapter III
searches the library for an artifact with mana cost `{0}` or `{1}` and puts it
onto the battlefield. Then the Saga is sacrificed. For a Lantern deck that is
the one route to the Lantern that costs no mana at all, and until this shipped
it was stood in for with *Urza's Saga drawn by turn 3* — an upper bound, because
chapter counters did not exist.

It is a tutor that waits:

```toml
[[effect]]
match = "name:\"Urza's Saga\""
on = "landdrop"
after = 2               # whole turns between the drop and the effect
sacrifice = true        # the Saga leaves the battlefield once it has resolved
fetch = ['name:"Lantern of Insight"']
to = "battlefield"

[land_drop]
prefer = ["name:\"Urza's Saga\"", "t:land"]
```

**When it fires is the whole of it.** Two turns after the drop, *after* that
turn's draw step and *before* its land drop, because a lore counter goes on as
the precombat main phase begins. So a Lantern drawn on the turn chapter III
resolves is already in your hand and not in the library for it to find — and
the route is exactly *the Saga in play by turn t, and the Lantern still in the
library on turn t+2*.

**What it moves.** The Lantern leaves the library and arrives **beside** the
Saga, not in place of it the way a fetchland's land does. A `zone =
"battlefield"` question about the Lantern is answered for this reason alone: a
permanent a delayed fetch can put there is one this walk counts arriving, and
so is one the `[casting]` line casts — so *the Lantern in play by turn 5* is
one question whichever way it got there. Anything else on the battlefield that
is not a land is still refused.

**What it costs.** `sacrifice = true` takes the Saga off the battlefield at the
end of the turn it resolves. Its `{C}` is still that turn's — you tap it with
chapter III on the stack, and the mana stays in the pool for the main phase —
and no later turn's. It is not counted in the graveyard, which is the stance a
cracked fetchland takes too.

**Why it stays exact.** Nothing is revealed and nothing branches: a delayed
fetch is the same subtraction from a named group an immediate one is, taken on
a later turn. A delayed *look* would need a checkpoint on a turn the schedule
cannot know, and it is refused.

On `decks/lantern.deck.toml`, exact, in 0.05s:

| | play | draw |
|---|---|---|
| Urza's Saga drawn by turn 3 (the old stand-in) | 9.09% | 10.10% |
| Lantern onto the battlefield by it, turn 3 | 6.49% | 7.34% |
| ...turn 5 | **8.32%** | **9.14%** |

Checkable by hand — one Saga and one Lantern in 99, so on the play by turn 5 it
is (7 × 90 + 89 + 88) / (99 × 98). `decks/lantern-route-c.criteria.toml` is
this question on its own; `decks/lantern.criteria.toml` wrote the same route as
clauses with no effect declared until it became one line (#100), and the two
agree to the last digit — the clause phrasing is kept as a test fixture,
`saga-gates.criteria.toml`, asked of the real list. HANDS.md hand 17 works it on
a sixteen-card deck.

What it does not model: the Saga surviving two turns of an opponent, chapter
III's other targets, and the shuffle — a card an earlier surveil left on top
stays on top, which the tutors above already do.

### An activation the line pays for: Expedition Map

Expedition Map is a `{1}` artifact; `{2}`, `{T}`, sacrifice it: search your
library for a land card and put it into your hand. For a Lantern deck the land
is Urza's Saga, whose chapter III then goes and gets the Lantern. The Map is
cast by the line like any card, and its ability is an effect with `on =
"activate"`, whose `cost` is what **activating** it costs — the Map is still
cast for the `{1}` printed on it
([ADR-0019](docs/adr/0019-a-tutor-route-is-something-the-line-pays-for.md)):

```toml
[[effect]]
match = 'name:"Expedition Map"'
on = "activate"
cost = "{2}"
sacrifice = true        # paying it takes the Map out of play
fetch = ["name:\"Urza's Saga\""]
to = "hand"

[land_drop]
prefer = ["name:\"Urza's Saga\"", "t:land"]

[casting]
prefer = ['name:"Expedition Map"']
```

**One entry, both payments.** An activation is not a list of its own: the
`[casting]` entry that names the Map, reached by the line, first pays to
activate a copy the line already put into play, then casts a copy from hand,
and the line is read again from its top after each — so a Map cast this turn is
activated this turn if the pool still pays, because an artifact has no
summoning sickness. At most once per permanent per turn, because its cost taps
it; a sacrificed one is gone, and counted nowhere, as a Saga after its last
chapter is. A `cast` clause still counts the Map's casting.

**After the land drop, with one exception.** The line runs after the drop, and
so does an activation — unless what it fetches is a land the `[land_drop]` list
ranks above every land in hand. Then it is paid **before** the drop, out of the
lands and rocks already in play, and the drop plays what it fetched: a Map cast
on turn 2 is activated on turn 3 and the Saga is turn 3's land, where paid
after the drop it would wait for turn 4. Nothing else is paid before the drop,
and the drop made after it pays only for what the line casts after it: its mana
was not there to pay the activation. The run says all of this beside the
effect:

```
note: effect "name:\"Expedition Map\"" (on activate)
      applies to 1 card: Expedition Map
      and the [casting] entry naming it pays {2} to activate a copy it put into play, sacrificing it, once a turn, after the land drop — before it only where what it fetches is a land [land_drop] ranks above every land in hand
```

and the JSON's effect carries `on = "activate"`, the `cost` and `sacrifice`.
An activation on a creature is refused by name, because its `{T}` would wait
out summoning sickness, and so is one on a land, whose ability is paid out of
the drop the line reads; neither is a tutor this deck needs. `after` and `look`
are refused on an activation as they are on a cast, and a line must be declared
for it to fire.

**What it moves.** HANDS.md hand 42, five Islands, eight Bolts, the Map, the
Saga and the Lantern, sixteen cards on the play, the line naming only the Map:

| | Map never activated | activation `{2}`, Saga to hand |
|---|---|---|
| Expedition Map cast on turn 1 | 4921/11440 = 43.02% | 4921/11440 = 43.02% |
| Urza's Saga played by turn 3 | 9/16 = 56.25% | **26249/34320 = 76.48%** |
| Lantern on the battlefield by turn 5 | 1/4 = 25.00% | **3977/12870 = 30.90%** |

**On the deck it was built for.** `decks/lantern-route-map.criteria.toml` is
the tutors-route file with the Saga and the Map added — a file of its own,
because the Saga needs a declared land drop, and declaring one moves the other
file's numbers for reasons that are not the Map's. Against the same file
without the Map:

| On `decks/lantern.deck.toml` | play, no Map | play, + Map | draw, no Map | draw, + Map |
|---|---|---|---|---|
| Lantern on the battlefield by turn 5 | 26.93% | **31.64%** | 30.38% | **35.95%** |
| Urza's Saga played by turn 3 | 9.18% | 14.77% | 10.24% | 16.95% |

Sampled, ± 0.06 to 0.11: 24 groups at turn 5. +4.71 points on the play and
+5.57 on the draw, against ADR-0019's +3.25 and +3.52, which were measured with
Trinket Mage, Fabricate and Cruel Captain already in the line.

### An activation whose cost discards: Artificer's Intuition

Artificer's Intuition is a `{1}{U}` enchantment: `{U}`, discard an artifact
card: search your library for an artifact card with mana value 1 or less and
put it into your hand. It is an activation like the Map's, and the discard is
**part of its cost** ([ADR-0019](docs/adr/0019-a-tutor-route-is-something-the-line-pays-for.md)
§4), written with the same keys a cast's discard uses:

```toml
[[effect]]
match = '''name:"Artificer's Intuition"'''
on = "activate"
cost = "{U}"
discard = 1             # the card fixes how many
discard_only = "t:artifact"   # and which cards may go
fetch = ['name:"Lantern of Insight"']
to = "hand"

[discard]
prefer = ['name:"Codex Shredder"']   # which of them you pay with
```

- **Paid before the search, or not at all.** Every part of a cost is paid as
  the ability is activated (CR 602.2b, 118.3). So the discarded card is gone
  before the search, and with no card to discard there is no activation: no
  `{U}` spent, nothing fetched. Frantic Search's discard is an effect and
  resolves with whatever is in hand; this one is a condition on activating.
- **Only a card `[discard] prefer` names pays.** It is the same list every
  claimant on the hand reads, and there is no list of its own. A forced
  discard falls back on the cards no entry names, but a cost does not: a
  line that could pay with any artifact card would pay with the Lantern it is
  looking for. A tie inside one entry is priced every way it can fall. With
  no `[discard]` list the run is refused by name, and a list that can take an
  artifact land needs `[land_drop]`, as a cast's discard does.
- **An enchantment, activated the turn it is cast.** Nothing in its cost taps
  it. The cost has no `{T}`, so a pilot could activate it twice in a turn. The
  line activates a permanent once a turn, which is a floor, and an exact one
  for a fetch that names only the Lantern. It is paid after the land drop,
  never before: which lands are still in hand is what the drop decides.
- A declared line activates whenever the pool and the list pay, even with
  nothing left to find, as it does the Map.

`draw`, `untap`, `at_random` and `discard_any` are refused on an activation,
because a cost is paid with cards the pilot chooses. The run says it beside
the effect:

```
      and the [casting] entry naming it pays {U} to activate a copy it put into play, once a turn, after the land drop
      and discards 1 matching "t:artifact" as part of that cost: only a card the [discard] list names pays it, and without one it is not activated
```

**What it moves.** HANDS.md hand 61: ten Islands, Intuition, Codex Shredder
and the Lantern, thirteen cards on the play, the line naming the Lantern and
then Intuition:

| | never activated | `prefer = ['name:"Codex Shredder"']` | `prefer = ['t:land']` |
|---|---|---|---|
| Intuition cast by turn 2 | 89/156 = 57.05% | 89/156 = 57.05% | 57.05% |
| Lantern on the battlefield by turn 4 | 10/13 = 76.92% | **265/286 = 92.66%** | 76.92% |
| Codex Shredder in the graveyard by turn 4 | 0 | **15/26 = 57.69%** | 0 |

**On the deck it was built for.** `decks/lantern-route-map.criteria.toml` now
casts and activates Intuition last in its line. Its `[discard]` list pays with
any artifact card the line never casts that is not a land drop, so never the
Lantern or the Map:

| On `decks/lantern.deck.toml` | play, before | play, + Intuition | draw, before | draw, + Intuition |
|---|---|---|---|---|
| Lantern on the battlefield by turn 5 | 31.64% | **37.31%** | 35.96% | **42.24%** |
| Lantern of Insight cast by turn 5 | 15.45% | 21.56% | 17.34% | 24.13% |

Sampled, ± 0.08 to 0.11: 30 groups at turn 5. That is +5.67 points on the play
and +6.29 on the draw, against ADR-0019's +3.10 and +3.60, which were measured
with more routes in the line for Intuition to overlap with. A discard can split
the path, so the run counts the walk before it samples. That takes about 60s a
seat where the file took 2s, as `loam-cast.criteria.toml` pays for its
discards.

### The Lantern north star, as one line

`decks/lantern.criteria.toml` is the north star as its owner defines it
([#100](https://github.com/cramt/progress-engine/issues/100); CONTEXT.md,
*North star*): **Lantern of Insight on the battlefield and Rashmi and Ragavan
cast, by turn N**, by any line the pilot's own cards allow, out of one mana
budget, with the opponents out of scope. Each turn is one criterion of two
clauses, and every route to the Lantern is the effect that is the route rather
than a clause standing in for it: the Lantern cast; Trinket Mage, Fabricate and
Tezzeret, Cruel Captain to hand; Whir of Invention at X = 1 and Tezzeret the
Seeker onto the battlefield; Dizzy Spell's transmute; Urza's Saga's chapter III;
Expedition Map for the Saga; Artificer's Intuition, its discard paid from a
`[discard]` list of the sixteen artifact cards the line never casts. The rocks
of ADR-0018 are in the same line.

| On `decks/lantern.deck.toml` | play | draw |
|---|---|---|
| north star by turn 4 | 7.71% ± 0.06 | 9.83% ± 0.07 |
| north star by turn 5 | **33.58% ± 0.11** | **40.94% ± 0.11** |
| north star by turn 6 | 47.06% ± 0.11 | 55.22% ± 0.11 |
| north star by turn 7 | 55.71% ± 0.11 | 62.88% ± 0.11 |
| the Lantern half alone, turn 5 | 53.64% ± 0.11 | 59.98% ± 0.11 |
| the commander half alone, turn 5 | 61.48% ± 0.11 | 68.43% ± 0.10 |

Sampled: each turn is a class of 39 groups, 104,983,073,472,420 compositions
at turn 5 on the play, and the discard makes the run count its walk before it
samples, about 7.5 minutes a seat (6 seconds before Intuition joined). No
bound: the owner chooses one, and its turn, from the curve. Without Intuition
the line read 31.09% / 38.18% at turn 5.

**The line** casts the Lantern first, then Sol Ring (which pays for what follows
it the same turn), then Rashmi the moment the pool pays, then the tutors — the
two that land the Lantern, then the three-mana hand tutors, then Dizzy Spell —
then the Map, then the two-mana rocks, then Intuition, which is half a point
better last than after the other tutors. Where the commander sits is the one
choice that measurably moves the answer: behind every tutor it costs 3.8 points
on the play and 5.5 on the draw. **The land drop** plays the Saga the moment it
is held, then the lands that enter tapped, then the untapped ones, most colours
first within each, one kind of land per entry; tapped-first is worth
five to six points over playing any land after the Saga. The file lists both
choices against the alternatives it was measured beside, and everything it
does not model with the direction each moves the number — digging, the three
refused tutors, improvise, the Treasure, Intuition searched once a turn and the
pilot adapting all make it a floor.

It is not the number the file used to report. That was *a line that resolves
Lantern of Insight by turn 5*, 47.10% / 54.34%: no commander, lands only, and
every gate reading the lands in whichever order suited it. `checker/` plays the
new line and land drop from the rules a turn at a time (`drop_line_path`, held
to HANDS.md hands 12, 17, 42 and 61 by `checker/test_land_drop.py`) and holds every
turn of the curve, both seats, to the engine's answer.

### Mulligans

Every number this tool printed before `[mulligan]` existed assumed you keep
whatever seven you are dealt. Nobody plays that way, and the decks this tool was
built for play it least of all, so "commander on turn 2: 44.29%" was a precise
answer to a question nobody asked. The fixture's own *keepable opener*
criterion shows the workaround — a keep rule wearing a criterion's clothes,
printed beside numbers that kept every seven anyway. Declared as what it is:

```toml
[mulligan]
keep = [{ query = "t:land", min = 2, max = 5 }]
bottom = ["t:land"]
down_to = 5
```

```
$ gauntlet test simple-ramp.txt simple-ramp.criteria.toml   # with the table above
note: every number below is of the hand the mulligan this file declared keeps.
      A hand is kept when it holds 2 to 5 of "t:land".
      After a mulligan, cards go back in this order:
      1. "t:land"
      then a card this list does not name goes back after every card it does.
      Ties: a tie inside one entry is settled at random, and every way it could fall is priced.
      A hand of 5 is kept whatever it holds.
      Kept at 7 cards 78.97%, 6 cards 10.52%, 5 cards 10.50%.
      Each criterion also shows, in brackets, its number had every first seven been kept.
PASS keepable opener (2-5 lands)   91.76%  (needs 70.0%)  [keep 7: 78.97%]
PASS turn-1 accelerant             47.14%  (needs 35.0%)  [keep 7: 51.04%]
PASS commander on turn 2           46.16%  (needs 30.0%)  [keep 7: 44.29%]
     any ramp by turn 3            93.46%  [keep 7: 94.39%]
```

Three parts, and every one is required, because every one is a decision about
how the pilot plays and a default for any of them would be the tool making it:

- **`keep`** is count clauses — `query`, `min`, `max`, the same as any clause —
  and a hand is kept when all of them hold. They are read of the hand you would
  *keep*, after bottoming: at six, "two to five lands" is a statement about six
  cards. There is no `turn` and no `zone`, because a keep decision is only ever
  about the opener, and writing either is refused by the schema.
- **`bottom`** is a declared priority over queries, the fifth resource on the
  mechanism `[land_drop]`, `[casting]` and a tutor's `fetch` already use: keeping
  at depth *d* puts *d* cards back, taken from the first entry the hand holds,
  then the next. A card no entry names goes back after every card one does,
  because a hand that has to put three back puts three back.
- **`down_to`** is the smallest hand you will go to, kept whatever it holds.
  Without it, a rule no hand passes would mulligan into nothing.

**It stays exact.** Under the London mulligan every redraw is a fresh deal of
seven from the whole library, so the depths are independent and the answer is a
sum of enumerations the engine already knew how to do:

```
P(C) = Σ_d  P(reach d) · P(keep at d, and C | dealt at depth d)
P(reach d+1) = P(reach d) · (1 − P(keep at d))
```

The walk is split at the opener, because that is where a mulligan decides
anything, and a hand the rule throws back is not dealt past it. Kept at seven is
exactly the old *keepable opener* criterion, 78.97%, because at seven nothing has
gone back; the rest is checkable by hand, and `cli.rs` checks it.

**A tie inside one `bottom` entry is priced, not broken.** Every other list on
this mechanism settles a tie by the card the decklist names first. Here that rule
would be unsound: each class of question is enumerated on the coarsest grouping
that can tell its cards apart, and merging two lands a "decklist first" rule
ordered differently puts back a different land — a narrowing that changes the
answer. A card chosen uniformly among the ones an entry cannot separate is the
one rule that survives merging, because a hypergeometric over merged groups is
the marginal of the one over the groups themselves. So the enumeration walks every
way the coin could fall, weighted by its chance, and the sampler tosses it. The
property test that holds narrowing to the unnarrowed answer fails against the
"decklist first" rule and passes against this one. A pilot who cares which of two
lands goes back names one of them in an earlier entry.

**Turn 0 is the hand you kept** — five cards after a mulligan to five, not the
seven it was dealt from. The cards that went back are on the bottom of the
library, which is where every count of the library already finds them; a tutor
searching the library still finds one, last, since it is the copy no draw would
ever have reached.

**The verdict is the mulligan's, and the seven's number sits beside it.** The two
differ by enough that switching from one to the other silently would look like
the deck changed, so every criterion carries both — `keep_seven` in the JSON,
`[keep 7: …]` on the line — from the same engine, and `at_least` is judged on the
mulligan's, because that is the question it was always asking. The `mulligan`
block in the JSON carries the rule, the list, and the share of games kept at each
hand size. A file with no `[mulligan]` gets exactly the numbers it always got, and
says, above them, that it kept every seven.

**Measured on the real decks,** with the rule above appended to each committed
file: kept at seven 83.96% on `decks/lantern.deck.toml` and 87.47% on `decks/loam.deck.toml`.
Every exact criterion's keep-seven number is the old number to the last digit. No
class crossed the ceiling it was not already over: `t:land` was already in the
widest classes, and the others grow by one group — a cumulative class that read
12 compositions reads 540, because the opener is now its own checkpoint. What the
mulligan costs is **deals**, one per hand size, and every entry in the
`enumerations` block says how many. Every deal is split at its opener and
walked on every core ([threads](#threads)), so on a four-core machine the loam
file with the rule above takes 0.8s on the play and 1.6s on the draw, and the
lantern file 0.5s and 0.6s — against 3.7s and 7.5s, and 1.1s and 1.5s, before.

Not modelled, and named rather than approximated:

- **The shuffle.** A fetch or tutor shuffles the library, which would return the
  cards you put on the bottom to the pile the next draw comes from. Here they
  stay on the bottom, which is the stance the tool already takes of a card an
  earlier surveil left on top.
- **A keep rule that is a disjunction.** `keep` is a conjunction; "a Lantern
  *or* a way to find one" is not writable yet.
- **Bottoming by what the hand needs, in a declared rule.** The list is a
  fixed priority, so `bottom = ["t:land"]` puts lands back even from a hand with
  two of them, which is why the floor above keeps 5.2% of hands with no land at
  all. A strategy the tool [chooses](#choosing-the-mulligan) puts back whatever
  serves its objective, hand by hand.
- **Free mulligans, Serum Powder, Commander's old rule.** The depths are a
  sum, so a free mulligan is a shift in them rather than a new mechanism; it is
  not built.

### Choosing the mulligan

A declared rule is the pilot's strategy. The other question is **what the best
strategy is**, for the questions a deck is being built to answer, and how much
better it is than the one declared
([#63](https://github.com/cramt/progress-engine/issues/63)). "Best" means
nothing until it says what for, so the file says — criteria by name, and what
each is worth:

```toml
[mulligan]
optimise = { "commander on turn 2" = 3, "keepable opener (2-5 lands)" = 1 }
down_to = 5
```

A game scores the weights of the criteria it met, and the strategy maximises the
expected score. Only the ratios matter: `3 : 1` says a commander on turn 2 is
worth three games that merely kept a keepable hand. That is taste, so the file
declares it and the tool does the arithmetic. Someone who wants two things
*together* writes the conjunction as its own criterion and weighs that.

```
$ gauntlet test simple-ramp.txt optimise.criteria.toml
note: every number below is of the hand the mulligan chosen for this objective keeps:
      1 × "keepable opener (2-5 lands)"
    + 3 × "commander on turn 2"
      Expected score 3.3281 of 4.
      Keep 7 cards scoring at least 2.8794.
      Keep 6 cards scoring at least 2.1274.
      A hand of 5 is kept whatever it holds.
      Cards go back the way that scores best. Ties: ways that score the same are all taken, each card with the chance a uniform choice gives it.
      Kept at 7 cards 40.16%, 6 cards 24.03%, 5 cards 35.81%.
                                     chosen   alone
      "keepable opener (2-5 lands)"   92.65%   99.14%
      "commander on turn 2"           80.05%   80.05%
      A strategy values only what this run models: a hand whose strength is a card this
      tool cannot yet play — a cantrip, a cycling land — is undervalued by it.
PASS keepable opener (2-5 lands)   92.65%  (needs 70.0%)  [keep 7: 78.97%]
PASS turn-1 accelerant             82.46%  (needs 35.0%)  [keep 7: 51.04%]
PASS commander on turn 2           80.05%  (needs 30.0%)  [keep 7: 44.29%]
     any ramp by turn 3            96.68%  [keep 7: 94.39%]
```

**It is exact, and it is a threshold per depth.** A kept hand's value is
linear — each weighted criterion's chance given that hand and what went back —
and each of those conditionals is the rest of the game dealt from the library
the hand left, which is the walk the engine already does, split at the opener.
Under the London mulligan every redraw is a fresh deal, so backward induction
over the depths is the whole search:

```
V_floor = E_h[ value(best_bottom(h)) ]
keep h at depth d  ⇔  value(best_bottom(h)) ≥ V_{d+1}
V_d     = E_h[ max(value(best_bottom(h)), V_{d+1}) ]
```

The thresholds printed are the V's: keep a seven worth at least 2.8794 points,
because that is what mulliganing it is worth. `best_bottom` tries every way of
putting the cards back, so a land-light hand at six puts a spell back and a
flooded one puts a land back — the thing a fixed `bottom` list cannot do.

**Every number is under the strategy, not only the ones it was chosen for.**
*Turn-1 accelerant* was not in the objective and still moved from 51.04% to
82.46%, because it is dealt the hands the strategy kept
([#64](https://github.com/cramt/progress-engine/issues/64)). A class reads the
opener on the join of its own grouping and the strategy's — seven cards, so the
join is small — and plays the rest of the game on its own grouping, where the
library is the deck minus that hand. The sampler plays the same table game by
game and the two engines agree; the optimiser's own number for each objective
criterion and the run's number for it are two computations, and they agree to
the last printed digit on both decks and both seats in `decks/`.

**What each weight costs is printed.** Beside each objective criterion is what it
would get if the strategy served it alone: *keepable opener* gives up 6.5
points so that the commander line keeps all of its. A weight that does not say
what its writer meant shows up here first.

**Beside a declared rule, both.** A file with `keep` and `optimise` plays the
declared rule — it is the pilot's strategy — and reports the chosen one beside
it, with each objective criterion under both and the gap as a score: on the
table above, 3.3281 chosen against 2.3024 declared.

**A sampled criterion cannot enter the objective,** and is refused by name. An
optimum found over sampled conditionals keeps the openers that were lucky in the
sample and reports a score that is too high, with nothing in the number to say
so. The Lantern north star is refused this way today. Nor may the optimiser run
away: pricing every opener and every way of putting back is capped at twenty
times the enumeration ceiling, and `walked` in the JSON says what it cost.

**The whole strategy is in the JSON** — `optimised.strategy`, every opener the
strategy tells apart with its chance and, per hand size, whether it is kept, what
it scores and what goes back — so a reader can check the choice rather than
trust it.

**Measured on the real decks**, with objectives made of criteria each file
already answers exactly (the north stars are sampled, so they cannot be weighed
yet). On `decks/lantern.deck.toml`, routes 1, 2 and 4 weighted 3 : 1 : 1: 6 groups and
338 openers, 134,469 paths walked on the play and 535,727 on the draw, 0.8s and
1.0s for the whole file on four cores. On `decks/loam.deck.toml`, Loam in hand, the
`{1}{G}` control and Loam Access with three lands weighted 3 : 1 : 1: 7 groups and
1,253 openers, 2.0 million paths on the play and 10.2 million on the draw, 1.0s
and 2.5s on four cores, 5.7s and 11.0s before the walk was split across them. Both
strategies mulligan hard — the Lantern one keeps 13.7% of sevens — because
nothing in either objective charges for a smaller hand. That is what the
objective said; a criterion like *four cards in hand on turn 5*, weighted, is how
a file says otherwise.

**A strategy values only what this run models.** A hand whose strength is a card
this tool cannot yet play — a cantrip, a cycling land — is undervalued by it, and
every run that chose one says so beside its thresholds.

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
$ gauntlet test simple-ramp.txt lands-by-turn.criteria.toml
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
$ gauntlet sync
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
| `!"name"` | The whole name, or one face of a multi-faced card; `!"Sol Ring"` is that card and no other |
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

**A term about a printing is refused here, by name.** `is:fullart`, `lang:ja`,
`frame:showcase`, `border:gold`, `st:masterpiece`, `game:paper`, `cn:263` and the
promo-type words (`is:ub`, `is:sourcematerial`, `is:poster`) describe one
printing, and the index holds one record per card, so a criteria file asking
one is told it cannot be answered instead of counting nothing. They exist for
Meldweb Curator, which ranks the printings of a card by them
([ADR-0026](docs/adr/0026-which-printing-comes-first-is-a-ranked-list-of-printing-queries.md)).
Each reads the field Scryfall reads (`is:sourcematerial` is the
`sourcematerial` entry in `promo_types`), and they are checked against every
printing Scryfall has of three cards, not against the whole pool:
`chip-scryfall/tests/fixtures/printings.sh` rebuilds that sample.

**`otag:` is how the curated lists get here.** Scryfall's oracle tags — what a
card *does*, as opposed to what it says — are not in the bulk data. They are not
derivable from it either: a naive `/enters.*tapped/` calls a Temple and a
Shockland the same thing, and "enters tapped unless you control two or fewer
other lands" is conditional in a way no regex survives.

So they are not derived. They are **fetched**, from Scryfall's search API at
`sync` time, and the index records the date it asked. That is the same standard
the rest of this file is held to: not a hard-coded copy that goes stale, and not
a guess dressed as a fact, but somebody else's answer with a date attached.

`sync` fetches ten tags today, each because something here reads it:

| Tag | Cards | Read by |
|---|---|---|
| `tapland` | 493 | whether two lands are actually two mana |
| `conditional-tapland` | 179 | the half `tapland` is not: a shockland enters tapped only if you decline to pay, so the gate has to say which way it read the choice |
| `surveil` | 332 | how many cards deep a turn sees |
| `scry` | 462 | the same, leaving the card on top |
| `mill` | 1,294 | the graveyard as a destination |
| `tutor` | 1,163 | selection over the whole library |
| `ramp` | 2,286 | the mana-curve questions |
| `fetchland` | 54 | deck thinning: the cards that remove a land from the library rather than looking at one. `is:fetchland` is the ten-card cycle; this is Prismatic Vista and Terramorphic Expanse too, and no query over card text separates them |
| `mana-rock` | 384 | how much mana a rock the line cast adds: the standard library's `adds` entries |
| `mana-dork` | 441 | the same, for creatures |

They cost nothing to carry: 5,301 of 35,004 cards are tagged, the file is the
same 24MB, and a run parses only the cards your deck names either way.

**An index carries the tags it was told to fetch, and says which.** The header
lists them, so `otag:` can tell *this index never asked about that tag* apart
from *no card is in it* — the same empty result, and very different facts. A
query naming a tag the index does not carry is an error naming the tag, for the
same reason `kw:tramp` is.

There are three ways an `otag:` question comes back with no cards, and they are
three different answers:

```
$ gauntlet test loam.deck.toml mill.criteria.toml
Error: mill.criteria.toml: a mill card in the opener: in query "otag:mill": this index does not carry otag:mill, so counting it would be zero by construction
      rather than by measurement. This index carries: scry, surveil, tapland.

$ gauntlet test simple-ramp.txt surveil.criteria.toml     # an index built with --from
Error: surveil.criteria.toml: surveil lands in the opener: in query "t:land otag:surveil": this index carries no oracle tags at all, so otag:surveil would match nothing here
      whether or not this deck plays such a card.
      `sync --from` builds an index like this one: tags come from Scryfall's search API,
      not from the bulk file. Fetch them with: gauntlet sync

$ gauntlet test loam.deck.toml scry.criteria.toml
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
$ gauntlet test simple-ramp.txt typo.criteria.toml
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
$ gauntlet test loam.deck.toml kw-typo.criteria.toml --index loam-index.jsonl
Error: kw-typo.criteria.toml: a typoed keyword: in query "kw:flyign": no card in this index has kw:flyign, so counting it would be zero by construction
      rather than by measurement — which reads exactly like a deck that plays none. The
      index lists every keyword the whole card pool carries, so this is a misspelling
      unless it is newer than the index. Check the spelling; rebuild with: gauntlet sync
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
$ gauntlet test unfinity.txt criteria.toml
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
$ gauntlet test all-commander.txt criteria.toml
Error: criteria.toml: the library is empty: every card in the list is a commander or outside the deck

$ gauntlet test two-card-deck.txt criteria.toml
Error: criteria.toml: this question draws 7 cards from a library of 2
```

The second is refused identically under `--simulate`. Left to themselves the two
engines disagree by the whole answer: enumeration finds no dealable hand and
reports 0%, while sampling deals what it can and reports whatever that gives.

Neither falls back to sampling the way a
[too-wide question](#when-the-question-is-too-wide) does, and the difference is
the point: a question that was only expensive still has an answer worth
estimating, and one that was never modelled does not.

## Deck format

`test` reads two formats: the family's own `.deck.toml`, and Archidekt's text
export. The committed decks are `.deck.toml`; `gauntlet import <file.txt>`
converts an Archidekt export into one.
[ADR-0020](docs/adr/0020-decks-are-toml-with-typed-categories.md) is the
authority on the format.

```toml
name = "Izzet Lessons"

cards = [
  { printing = "tla/46", in = ["learnboard", "tempo"] },  # Boomerang Basics
  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration
  { name = "Lightning Bolt", qty = 4, finish = "foil", in = ["tempo"] },
]

[categories]
learnboard = { type = "sideboard" }
tempo = {}
```

- **A card is named exactly once**, by `printing = "set/number"` or by
  `name`. A printing is resolved to its card through the index, which
  `gauntlet sync` fills from Scryfall's `default_cards`; an index without
  printings refuses a deck that needs them. The comment after a printing is
  written by the tools and never read.
- **Categories are declared**, so a misspelled one is an error. A category
  may be typed: `commander` is in the deck; `sideboard`, `companion`,
  `maybeboard`, `attractions` and `sticker-sheet` are outside it. An untyped
  category is a label, and `cat:` matches any category a card is in.
- **Where a card is follows from its types**, the most specific one winning,
  so Lurrus in `companion` and `recursion` is a companion. Types from two
  branches, such as `commander` and `sideboard`, refuse the deck.

### Archidekt's text export

Quantity and name are the only required parts:

```
1x Sol Ring
3x Plains
1x Lightning Bolt (sos) 267 *F* [Interaction]
1x Sol Ring (c21) 263 *E* [Ramp]
1x Erase (Not the Urza's Legacy One) [Removal,Test]
1x Rashmi and Ragavan [Commander{top}]
1x Lurrus of the Dream-Den [Companion{noDeck}]
// line-leading double slashes are comments
```

Notes:

- **Multiple categories** are comma-separated inside the brackets. A comma
  inside braces separates flags: `Maybeboard{noDeck,noPrice}` is one category.
- **A printing is a set and a collector number.** A parenthetical with no
  number after it is part of the name, as in `Erase (Not the Urza's Legacy
  One)`, so a bare `(tdc)` stays in the name and fails to resolve out loud.
- **The finish marker** after the printing is `*F*` for foil or `*E*` for
  etched. Any other `*X*` is refused by name rather than read into the card's
  name.
- **A category's flags belong to the category, not the line**, as in Archidekt:
  `[Commander{top}]` on one line makes every `[Commander]` line a commander
  too, in `parse` and in an import alike.
- **`//` is a comment only at the start of a line.** Card names contain it —
  `Unstable Glyphbridge // Sandswirl Wanderglyph` — and treating it as an inline marker would
  truncate every double-faced card.
- **Malformed lines are errors, not silences.** A line that cannot be parsed names itself and
  its line number. The predecessor to this parser dropped them quietly, so a typo surfaced much
  later as a mysteriously wrong card total.
- **A card is placed by its first category alone, as Archidekt places it**
  ([archidekt-import-shapes.md](docs/research/archidekt-import-shapes.md)).
  `{top}` on that category makes the card a commander; `{noDeck}` on it, or
  the exact names `Sideboard` and `Maybeboard`, take it out of the deck. So
  `[Commander]` without `{top}`, `[Ramp,Commander{top}]`, `[Removal,Sideboard]`,
  `[sideboard]` and `[Companion]` are all in the deck, because Archidekt counts
  them there. `# Heading` lines give the cards below them that category first,
  and `# Commander` is a commander heading by itself.
- **Importing into a `.deck.toml`** types a category only where it comes first
  on some line. A card that lists a typed category later, where Archidekt
  ignores it, comes in without it, and the import names the line.
- **`outside` is a fact about the decklist, not about the cards.** It is what the
  list says — companions, sideboards, `{noDeck}` — and that is all `parse` can
  know, having no card index. A sticker sheet is outside the library too, but
  nothing in the text says so, so it leaves later, at `test` time, where card data
  is at hand, and is reported separately as `excluded`.
- **A line outside the deck is never looked up.** A `{noDeck}` token in an
  Archidekt export's `Tokens & Extras` category moves no probability, so it
  does not have to resolve. A line that is counted and cannot be resolved is
  refused with the remedy that works for it: `gauntlet sync` for a real card
  newer than the index, and `{noDeck}` for a token — the index holds no tokens,
  so no sync will ever find one. A real card filed under a token category, like
  Shapeshifter, is counted and named in a note, because the list may have
  meant the token.

## Development

```bash
cargo test --all   # plain cargo; rust-toolchain.toml picks the toolchain
cargo clippy --all-targets -- -D warnings
nix develop        # optional devshell with the toolchain, jq, cargo-nextest, node and pnpm
nix flake check    # what CI runs: fmt, clippy -D warnings, tests, build, checker, meldweb-web
python3 checker/compare.py   # the independent checker, against target/release/gauntlet

# Meldweb Curator, inside `nix develop`
pnpm install
pnpm dev           # the editor on localhost:5173; VITE_MOCK_GITHUB=1 runs it against a fake GitHub
pnpm check         # biome + tsc; `pnpm test` is vitest, `pnpm build` the site
```

### An independent checker

`checker/` is a small Monte Carlo checker in standard-library Python, written
from what each question means, the Magic rules and the assumptions this README
states — not from the engine's source. It shuffles the real libraries in
`decks/`, deals 400,000 games per deck and seat, answers a handful of the
committed criteria (a pure draw question, a battlefield land count, three
`can_cast` joints, Loam cast into the graveyard — drawn, or fetched by
Spellseeker — played a turn at a time, the Lantern put onto the battlefield by
Tezzeret the Seeker's loyalty ability or by Whir of Invention, or found by Dizzy
Spell's transmute, and the commander cast from the command
zone on each deck) and holds every answer the engine gave **exactly** against
the checker's 99.9% interval, exiting non-zero on any that falls outside. An
answer the engine **estimated** carries an error of its own, so it is held to
the interval of the difference instead — both errors, added in quadrature — and
marked `agree (engine sampled)`: a weaker check, of what a dealt game does
rather than of the enumeration, and the only one the commander and Spellseeker
questions admit, because they are too wide to enumerate. Loyalty abilities are
the checker's own reading of CR 606 — sorcery speed, once a turn, a `−X` paid
out of the loyalty the walker entered with — and `checker/test_seeker.py`
plays HANDS.md hands 40 and 44 through it over every deal. A transmute is its
reading of CR 702.53 — the cost the text names, a card of the same mana value,
to hand — and a chosen X of CR 107.3, and `checker/test_declared_cost.py` plays
hands 41 and 50 through them over every deal. An activation is its reading of
CR 602 and 118.3 — the cost before the colon paid in full, the sacrifice part of
it, an artifact free of summoning sickness — and `checker/test_activation.py`
plays hand 42 from Expedition Map's text over every deal, the payment before
the land drop included. A discard in a cost is its reading of the same rules.
With no artifact card in hand the cost cannot be paid, so there is no
activation. `checker/test_intuition.py` plays hand 61 from Artificer's
Intuition's text over every order of the library. It is still a check:
it is how the Spellseeker line found the engine holding a fetched Loam it could
have cast:

```
cargo build --release -p gauntlet-cli
python3 checker/compare.py                       # or --gauntlet PATH, or $GAUNTLET
python3 checker/compare.py --games 100000 --seed 7
python3 checker/compare.py --jobs 1                # one process, the same numbers
```

It deals its games across every CPU it may use (`--jobs`) while the engine
runs beside them, and the split changes no game: each seeded stream is cut into
runs of 5,000 at the generator's state where the run begins, so every number is
the one a single process would deal, to the game, and depends on `--seed` alone
(`checker/test_stream.py` holds a chunked stream to a whole one). On a
four-CPU machine shared with other work it took 283 seconds where one process
took 557, measured back to back. Where the engine's documented reading differs
from the game — shocklands assumed tapped, a bounce land is one mana, a
fetchland's target is still in the library to find — the checker implements the
documented reading and says so beside the code, so a disagreement is a finding
about the engine rather than about the two disagreeing on the premise. Adding a
question is one function of a dealt game and one entry in `QUESTIONS`, named
exactly as its criterion is.

Every question that casts goes through one line model in the checker
(`line_path`): the commander from the command zone, a tutor that fetches to
hand, and the line read again from the top after every cast. It also plays
**rocks and dorks** the way
[ADR-0018](docs/adr/0018-rocks-and-dorks-are-sources-the-line-casts.md) says,
from the rules rather than from the engine: a rock the declared line casts taps
that turn but pays only for what the line casts after it, a dork waits a turn,
the amount comes from the card's oracle text (Sol Ring `{C}{C}`, a Talisman one
of its two colours or `{C}`, Arcane Signet the commander's colours), and Fellwar
Stone and Lotus Cobra make nothing. `checker/test_rocks.py` holds HANDS.md hands
26 to 33 against it (`python3 -m unittest discover -s checker`), and
`compare.py` holds the engine's commander-with-rocks answers against it on both
decks and seats. Those are dealt 100,000 games rather than the default 400,000
(`Question.games`): the line model is slow in Python, and the engine samples
them anyway, so its own error bar is most of the interval.

A line whose file declares its land drop and its discard list is a different
line — the one the pilot played rather than the best one — so it has a model
of its own (`declared_line_path`): one land a turn by the declared list, the
lands on the battlefield paying and nothing searched for, and the whole hand
kept so a discard can take from it. Frantic Search, Izzet Charm and Desperate
Ravings draw and discard from their oracle text and the README's reading of
each; `checker/test_discards.py` holds HANDS.md hands 21 to 24 against it, and
`compare.py` holds `decks/loam.criteria.toml`, the Loam north star, against it
on both seats and at every turn it asks, with the commander, the dorks and
Lumra's mill in the line (`checker/test_north_star.py` pins those three).

The Lantern side has a declared-land-drop model too, `drop_line_path`,
written for #100 alongside `declared_line_path` rather than on it: the two
were built in parallel for the two north stars, and each knows the routes its
deck needs — the Loam one discards, mills and dredges, the Lantern one tutors
by card text, fires the Saga's chapter and pays activations. Folding them into
one is follow-up work; each is held to its own hands. `drop_line_path` plays
one land a turn, chosen by the file's `[land_drop]` list as README states it
(first entry held wins, a tie to the card the decklist names first, a look
that routes nothing breaking no tie), pays the line out of the lands it played
and the rocks it cast, fires Urza's Saga's chapter III two turns after the drop
and sacrifices it (CR 714), and activates Expedition Map for the Saga (CR 602,
118.3), before the drop when the Saga is what the drop would play, and
Artificer's Intuition, whose cost discards a card the file's `[discard]` list
names or is not paid at all (CR 118.3, 602.2b). Each route
is read from its card's text: a tutor to hand, a loyalty ability or a chosen X
onto the battlefield, a transmute. `checker/test_land_drop.py` holds it to
HANDS.md hands 12, 17, 42 and 61 exactly, and `compare.py` holds the Lantern north
star — every turn of its curve, its two halves and the hard-cast Lantern, both
seats — to it. `compare.py` reads each question's answer from the file the
question names, so two files may ask questions of the same name.

A question the engine cannot answer yet is marked `pending="#NN"` in
`QUESTIONS`: `compare.py` reports the checker's number, on `--pending-games`
games, and fails nothing on it. When the criterion exists and the engine
answers it, `compare.py` compares it and says to delete the marker.

The cargo workspace is Ichormoon Gauntlet, Reality Chip and `meldweb-wasm`.
Gitaxian Probe, under `crates/gitaxian-probe/`, is a workspace of its own
outside this one, because its native host links a prebuilt V8 and needs the
network to build; the flake builds only its web half. Its engine README says
how.

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
dragging in the others. Crates are grouped by which progress-engine product
owns them:

```
crates/ichormoon-gauntlet/{cli,criteria,toml,sim}    this tool
crates/reality-chip/{scryfall,decklist,stats}        the shared family core
crates/meldweb-curator/{wasm,web,worker,infra}       the deck editor: chip-decklist in wasm, a TypeScript app, its worker, and the OpenTofu for its domain
crates/gitaxian-probe/                               card scanning; a cargo workspace of its own
```

`reality-chip` is the part a sibling tool depends on, kept in its own directory
so that extracting it later is a subdirectory filter rather than a salvage job.
See [NAMES_FOR_FUTURE.md](NAMES_FOR_FUTURE.md).

| Crate | Responsibility | Knows about |
|---|---|---|
| `chip-stats` | Exact hypergeometric draw probabilities | Nothing. No Magic concepts at all. |
| `chip-decklist` | Parsing Archidekt decklists, editing a deck and a collection and saying which line holds a card, writing a deck as Archidekt, Cockatrice and Cardmarket import it, and reading other apps' collection exports | Decklist and export text, and the names of its printings, or Scryfall's answers about an export's rows, when handed them. No card data. |
| `chip-scryfall` | Card data, Scryfall bulk data and search syntax, the printing terms Curator ranks by, and the copy of Scryfall (`chip_scryfall::copy`, ADR-0031): its kept format, building it from the bulk files and answering lookups, search and quick add from it, and its storage policy (`copy::store`: two slots and a meta file written last, when a copy is due a look, a file the builder refused), with the reading, writing and downloading, and which printing comes first, left to the caller | Cards and their printings. No decklists, and no product's printing preference. |
| `gauntlet-criteria` | Grouping cards by query, applying effects, evaluating exactly | Counts, the zones they are counted in, and where a looked-at card goes. Not cards, and not where the questions came from. |
| `gauntlet-toml` | Reading a criteria file and answering it, and shipping the standard effect library | The criteria format, and counts. No cards. |
| `gauntlet-sim` | Sampling, validated against `chip-stats` | Shuffling. |
| `gauntlet-cli` | The `gauntlet` binary, and the library behind it that prepares a run in-process | All of the above. |
| `meldweb-wasm` | `chip-decklist` and `chip_scryfall::copy` for Meldweb Curator's browser editor, with the TypeScript types generated from its wire types, and `meldweb.toml`'s printing preference over `chip-scryfall` and its deck order, read, checked and written. The copy's wasm functions hold the worker's copy and hand it the default printing preference | Decklist text, the printings of one card it is handed to rank, and the one copy of Scryfall the worker holds. |

The seam worth knowing about is between `chip-scryfall` and `chip-decklist`: a query
can filter on `cat:"Exile Outlet"`, which is decklist data, not card data. Rather
than have the card crate depend on the decklist crate, `CardView` takes
categories as a plain `&[String]`. The CLI is what joins the two, which keeps
both halves independently testable.

The same seam decides where each kind of "outside the library" lives. Companions
and sideboards are decklist data, so `chip-decklist` answers those; sticker sheets
and attractions are card data, so `chip-scryfall` answers those. Neither crate
learns about the other, and `gauntlet-cli` applies both at the point where a decklist
entry finally meets its card.

Legality is on the card side of that seam and stays entirely there. Whether a
card is banned, whether it may be repeated at all, whether its type line puts it
in the command zone and whether its identity fits inside a given one are all
facts one card settles alone, so they live in `chip-scryfall::legality`, which is
what `f:`, `banned:`, `restricted:`, `is:commander` and `is:partner` read. The
half that needed the decklist — copy counts, which lines were nominated,
how many cards there are altogether — used to live in `gauntlet-cli` and has been
removed ([#42](https://github.com/cramt/progress-engine/issues/42)): selecting
cards by what a format says about them is a query, and pronouncing on a whole
list is not something a draw-probability engine has any business doing.

`chip-stats` deliberately has no idea what a card is. Its tests are pure
known-answer arithmetic, so a failure there is unambiguously a maths bug rather
than a card-data bug. The same reasoning puts the evaluator behind a trait in
`gauntlet-criteria`: the enumeration is tested with plain Rust closures, so a failure
there is an engine bug and a failure in `gauntlet-toml` is a criteria-format bug.
Keeping those distinguishable is worth the indirection.

Its vocabulary is **populations, groups, draws and removals**, and tutoring
added the last of those without adding a Magic concept. A removal is a
deterministic subtraction from a named group between two checkpoints, decided by
the caller from the path so far; the walk asks for one at every checkpoint and
deals the next gap out of `groups - drawn - removed`. The known-answer test for
it is two groups of two: draw one, remove one from group 0, draw one more, and
P(exactly one card of group 0 in hand) is 3/4 where the plain walk says 4/6.
Nothing in that signature knows what a tutor is, which is the condition
[#18](https://github.com/cramt/progress-engine/issues/18) set on itself.

A **sized gap** is the random counterpart ([ADR-0017](docs/adr/0017-a-spells-draw-is-a-deal-the-path-sizes.md)):
after each checkpoint, right after the removals, the walk asks the caller how
many cards the next gap deals, deals a non-zero answer as one more checkpoint,
and asks again; zero deals nothing and costs one composition. Its known answer
is two groups of two where the first card, if it is an A, is followed by two
more before the last: 5/6 for an A by the end. Such a walk has no closed-form
width, so a class with one is counted against the ceiling, capped, while every
other class keeps the static bound. The standard library's mills are the first
runs to deal one ([Mills](#mills-a-spell-that-turns-cards-over)); nothing draws yet.

`gauntlet-toml` holds the only `impl Evaluator`, and both engines take it through the
same trait. That is why swapping the criteria format out from under them was a
new crate and a deleted one rather than a change to either engine — and why
there is one evaluator serving both rather than two that can disagree.

`gauntlet-sim` exists to check `chip-stats`, not to replace it. Where both can answer,
they must agree — and that agreement is asserted at three levels: unit, through
the criteria layer both engines share, and end to end through the binary.
