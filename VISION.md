# Vision

## What this is

A draw-probability engine for Magic decks. You describe what your deck has to
do, and it tells you how often the deck actually does it — exactly, by
enumeration, not by shuffling a million times and hoping.

That is the whole product. Everything below is a consequence of taking it
literally.

## What it is not

**It is not a deckbuilder.** It does not suggest cards, rank them, or have
taste.

**It is not a legality checker.** It will not tell you whether your list is
legal. The math does not need a format — hypergeometric draw probabilities care
about library size and composition, and both come from your decklist rather than
from any rule set. Having opinions about Commander specifically made a Modern
list second-class for no mathematical reason. See
[#42](https://github.com/cramt/progress-engine/issues/42).

**It is not format-specific.** Commander, Modern, Legacy, a pile of cards you
made up — if you can write it down, it can be asked about.

**It is not a programming language.** Criteria are data. See
[The criteria are data](#the-criteria-are-data).

## The north stars

Two real decks. When the tool can answer both, it works. One of them it now
can.

**Lantern control.** *How often is there a line that resolves Lantern of Insight
by turn 5?*

This was recorded for a long time as *how often do I have Lantern of Insight by
turn 5*, which is a smaller and easier question, and stating it that way made
the north star look nearly finished when it is not. A Lantern pilot does not
draw the card; they assemble it by whichever of three routes turns up:

| Route | Needs |
|---|---|
| Lantern in hand, and one mana | mana as a **gate** |
| Trinket Mage resolved, fetching it — three mana by turn 4, or four by turn 5 so both are cast in one turn | mana as a **budget**, and tutors |
| Urza's Saga played by turn 3, ticking to chapter III | **no mana at all** — a land drop, a two-turn delay, and a tutor onto the battlefield |

Three consequences, all of which were invisible while the question was recorded
in its easy form.

**The filtering half is done.** A surveil that routes everything you do not want
to the graveyard digs one card deeper each turn it fires.

**The question is a disjunction, and a criterion cannot say "or"** — the routes
differ in turn, in zone and in what they require, so no card query expresses
them. That is [#49](https://github.com/cramt/progress-engine/issues/49), and it
blocks this north star independently of the mana.

**Urza's Saga is the cheap route and was underrated.** It is an Enchantment
Land, so it arrives on a land drop, and chapter III puts Lantern onto the
battlefield for nothing. It needs delayed effects and a tutor destination, and
no part of the mana model.

**Life from the Loam.** *How often is Loam in my graveyard by turn 5?* Needs the
graveyard to be a thing you can ask about, and needs surveilling a card into the
yard to count as a route there — because for this deck it is the *good* route,
not a consolation prize. **Answerable**, as of the effect library and land-drop
routing.

Loam needed no mana model and is done. Lantern needs one for two of its three
routes, and needs [#49](https://github.com/cramt/progress-engine/issues/49)
before any of them can be written down together.

[HANDS.md](HANDS.md) works both of them, and a dozen other cases, as concrete
seven-card hands with the answer written down.

That is a statement about these two decks, not a scoping decision: **a mana
model is wanted**, and is the largest single piece of unbuilt work. See
[Mana](#mana).

## The principles

These are not aspirations. Each one is already load-bearing somewhere in the
code, and each one is why some feature was refused.

### Never answer a question you did not model

The defining failure is a number that is confidently wrong. A query that is
valid and matches nothing, a keyword typo that returns 0%, an index silently
missing a field, a criterion that asks about something the engine does not
simulate — each produces a percentage that looks exactly like a real one.

So the tool refuses. `RunError::TooWide` refuses questions too wide to enumerate.
An empty library is refused rather than answered with 0%. A query naming a
keyword the index has never seen is refused by name. A query naming an oracle
tag this index never fetched is refused, because *nobody asked about that tag*
and *no card is in it* are the same empty result and very different facts.

### Ask, don't guess

Where data is not derivable, it is fetched and dated rather than approximated.

Land cycles are the worked example. `is:shockland` and `otag:tapland` cannot be
derived from oracle text — "enters tapped unless you control two or fewer other
lands" is conditional in a way no regex survives. The old conclusion was that
they could not be supported. The right conclusion is to ask Scryfall and record
the date of the answer, which is the same standard the card index is already
held to.

### Every number names its inputs

A percentage that moved since last week is useless on its own, because your
deck, your criteria, the card index and the tool itself each move it, and in the
output they are indistinguishable. So the run records all of them. Anything else
that can move a number — the effect library, notably — belongs in that list too.

### Exact, with a second implementation as the oracle

The engine enumerates. There is no shuffler and no sampler bias to chase. The
sampling engine exists to disagree with the exact one, and their agreement is
asserted at three levels. A feature that cannot stay exact is a feature that
needs a very good reason.

### Make invalid states unrepresentable

Preferred over documenting the trap. One query per clause rather than a sum of
counts, because `count(a) + count(b)` double-counts a card in both groups and
`a or b` does not. Two vocabularies rather than one shared type, because a
caller checking a tag against the keyword list would get a confident wrong
answer.

## The shape

### The criteria are data

A criteria file is TOML. Not a script, not a DSL, not JavaScript.

```toml
[[criterion]]
name = "loam in the yard by turn 5"
at_least = 0.80

  [[criterion.require]]
  turn = 5
  query = 'name:"Life from the Loam"'
  zone = "graveyard"
  min = 1
```

This matters for three reasons, in ascending order of importance.

It is **faster**: the predicate is evaluated once per composition, up to five
million times, and every one of those was a crossing into a JavaScript runtime.

It is **pure**: a script can call `Math.random()`, and hashing an impure file as
provenance is a promise that cannot be kept.

It is **analysable**: the queries and zones a file asks about are readable
without running it. That is not a nicety — the reason a too-wide refusal could
not name the queries the file asked for was that the only way to learn them was
to run the file, and it was refused partway through running. The declarative
format deleted that bug rather than fixing it, and the refusal now names every
query the file holds.

### The card matching is Scryfall's

Wherever a query selects cards, it is Scryfall syntax — `t:land`, `otag:surveil`,
`cat:"Ramp"`. Users already know it from the search box, and where a key exists
it means what Scryfall means.

This is also what makes the effect library small enough to maintain.

### Effects are declared per query, not per card

Nothing in any card data says Opt is *scry 1, draw 1*. The amounts have to be
declared by somebody, and the product requirement is that the somebody is not
the user.

Keyed by card, that library would be thousands of entries and stale on every set
release. Keyed by query, it is dozens, and a new printing that surveils is
covered the day it exists:

```toml
[[effect]]
match = "t:land otag:surveil"
look = 1
on = "landdrop"
```

The library ships with the tool and loads as a prelude — the same `[[effect]]`
syntax a brewer writes for the one niche card nobody thought of. Not special
machinery, just entries that load first.

Shipped, for the land-drop tier. What the library declares is what a card
*looks at*; where the looked-at cards go is declared in the file that asks the
question, because the same surveil land wants Loam in the yard for one deck and
on top of the library for another. An entry that named a destination would be
this tool answering something nobody asked it.

### One mechanism for selection

Scry, surveil, mulligan bottoming, tutoring and milling are one concept: **a
declared priority over a looked-at set, evaluated against the counts in hand.**

Building four of these is how they end up disagreeing with each other. It also
keeps the engine exact, because given the composition of what you looked at, the
choice is a deterministic function of counts.

The policy is a **router**, not a filter. Each looked-at card goes to a declared
destination, and a criterion asks about whichever destination it was routed to.
Framing it as keep-versus-discard is wrong: for a Loam deck the discard pile is
the win condition.

Shipped for the land-drop tier, which is the tier that needs no mana: `look = n`
examines the top *n* cards, `to_graveyard` names which of them leave, and the
rest stay on top and arrive in hand next turn. A card left on top is the card
the next draw takes, so a look that routes nothing is exactly a no-op — which
is the honest answer when nobody has said where the cards should go, and the
reason the shipped library can autoload without moving a single number.
Mulligan bottoming and the budget's spell priority are the same mechanism with
a different resource, and are not built.

### Mana

Two lands is not two mana, and holding six Opts is not casting six Opts.

Mana appears twice, and the two are not the same problem.

As a **gate**, it decides whether a criterion is satisfiable at all: *can I cast
a `{1}{W}{U}` three-drop on turn three?* This is not answerable from counts a
user writes, because one Hallowed Fountain counts toward both `produces:w` and
`produces:u` and a conjunction of independent counts is satisfied by a hand that
cannot pay. It is a bipartite matching, and the engine can solve it exactly per
composition while the user cannot express it — so it has to be a primitive.

As a **budget**, it is spent. An opening hand of one Island and six Opt casts
*one* Opt, because the first one consumes the Island. A model where an effect
fires whenever you hold the card overstates that turn sixfold, and the number
looks perfectly reasonable in a report.

The gate is cheap: lands in play is a function of the checkpoint path the engine
already walks. The budget is not, because an effect that draws makes *cards seen
by turn T* path-dependent and changes the shape of the enumeration rather than
the state carried through it. So the gate ships first and is worth having alone.

Which spell you cast when you cannot cast both is a **declared priority over
queries** — the same mechanism as mulligan bottoming and selection routing, gated
by a resource instead of by a looked-at set. Not a fourth policy language.

### Zones are the real question

"Did I find the card" is not a well-formed question. "Is the card in this zone by
this turn" is. Every number printed before zones existed silently meant *in
hand*, and for a graveyard deck that is the wrong question asked confidently.

A clause now names its zone, and silence still means `hand` so that no file
written before this moves. `graveyard` is askable, and reachable exactly when
some effect in the run routes a card there; in a run where none does it is
correctly empty and the run says so rather than letting that zero pass for a
measurement. `battlefield` is refused by name: it would have to know what you
could cast, and that is the mana model.

## Who it is for

People who build decks, not people who write config.

The text format is an **API**, not a user interface. The intended surface is a
graphical builder — Archidekt embedding this behind blocks — which means the
format has to round-trip cleanly, structured rather than stringly, so that a
saved file loads back into blocks without anyone reimplementing a grammar.

The day someone else's parser and ours disagree, a deck's numbers change and
nothing says why. That is the failure this tool exists to prevent, imported
through the front door.

## Decided

- Draw-probability engine; legality checking comes out ([#42](https://github.com/cramt/progress-engine/issues/42)).
- Format-agnostic. Zone annotations stay, because library size is math; format
  rules go.
- Criteria are TOML, structured, no expression language ([#44](https://github.com/cramt/progress-engine/issues/44)) — shipped.
- Oracle tags are fetched at sync time and dated ([#41](https://github.com/cramt/progress-engine/issues/41)) — shipped.
- Mulligan bottoming is a declared priority list ([#7](https://github.com/cramt/progress-engine/issues/7)).
- The effect library ships, autoloads as a prelude, and is keyed on queries
  ([#43](https://github.com/cramt/progress-engine/issues/43)) — shipped.
- Overlapping effects resolve last-wins, per card — shipped.
- Selection is a router over a declared priority, and the land-drop tier ships
  first because one drop a turn bounds it
  ([#17](https://github.com/cramt/progress-engine/issues/17)) — shipped. The
  mana-gated tier is refused by name until
  [#10](https://github.com/cramt/progress-engine/issues/10).
- The standard library declares what a card looks at and never where the cards
  go, because the destination is part of the question.
- A mana model is in scope, staged gate-first then budget
  ([#10](https://github.com/cramt/progress-engine/issues/10)).
- A criterion names the zone it asks about, silence means `hand`, and
  `battlefield` is refused until castability exists rather than approximated
  ([#40](https://github.com/cramt/progress-engine/issues/40)) — shipped.

## Not yet decided

- Whether a user can *disable* a stdlib effect rather than override it.
- What `turn 0` means after a mulligan to five.
- Whether the report shows the mulligan-adjusted number, the keep-your-seven
  number, or both.
- Whether the pod matters at all now that the tool is format-agnostic
  ([#22](https://github.com/cramt/progress-engine/issues/22),
  [#23](https://github.com/cramt/progress-engine/issues/23)).
