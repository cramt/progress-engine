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

Two real decks. When the tool can answer both, it works. It answers the first
as a floor — every route written down and priced, with named inputs it still
cannot count — and cannot answer the second, and both were briefly recorded
here as closer than they were, which is the reason this section now names the
deck rather than the question.

**Lantern control.** *How often is there a line that resolves Lantern of Insight
by turn 5?*

This was recorded for a long time as *how often do I have Lantern of Insight by
turn 5*, which is a smaller and easier question, and stating it that way made
the north star look nearly finished when it is not. A Lantern pilot does not
draw the card; they assemble it by whichever of three routes turns up:

| Route | Needs |
|---|---|
| Lantern in hand, and one mana | mana as a **gate** |
| Trinket Mage resolved, fetching it — three mana by turn 4, or four by turn 5 so both are cast in one turn | mana as a **budget**, and tutors — both ship |
| Urza's Saga played by turn 3, ticking to chapter III | **no mana at all** — a land drop, a two-turn delay, and a tutor onto the battlefield |

Three consequences, all of which were invisible while the question was recorded
in its easy form.

**The filtering half is done.** A surveil that routes everything you do not want
to the graveyard digs one card deeper each turn it fires.

**The question is a disjunction, and a criterion can now say "or"** — the routes
differ in turn, in zone and in what they require, so no card query expresses
them. That was [#49](https://github.com/cramt/progress-engine/issues/49), and
`any_of` ships: the three routes are writable in one criterion, and the answer
is the union of them rather than a sum nobody could have taken safely. It no
longer blocks this north star. What is left is the mana.

**Urza's Saga is the cheap route and was underrated.** It is an Enchantment
Land, so it arrives on a land drop, and chapter III puts Lantern onto the
battlefield for nothing. It needed delayed effects and a tutor destination, and
no part of the mana model — and both ship: an `[[effect]]` with `on =
"landdrop"`, `after = 2` and `sacrifice = true` fires two turns after the drop,
after that turn's draw step, and takes the Saga and its mana off the
battlefield once it has. HANDS.md hand 17 is the route worked out on paper.

The first route is now writable: *Lantern in hand and one mana* is a clause
asking for the card and a clause asking `can_cast = "{1}"`, which is exactly the
gate — and it is writable **beside the deck's filtering** rather than in a file
of its own, now that the land drop those two argued over is declared rather than
refused ([#54](https://github.com/cramt/progress-engine/issues/54)). HANDS.md
hand 12 is that route worked out, and it prices the trade the deck actually
makes: playing the surveil land first halves the turn-1 number and wins the
turn-2 one.

**The second route is now writable end to end.** `[casting] prefer =
['name:"Trinket Mage"', 'name:"Lantern of Insight"']` spends the pool the way
the pilot would and `cast` counts what it paid for; an `[[effect]]` with `on =
"cast"` and `fetch = ['name:"Lantern of Insight"']` says what the spell then
went and got. On `decks/lantern.txt` Trinket Mage resolves by turn 3 on 5.03% of
hands, exactly, on 84,084 compositions — **unchanged by the fetch, and it has to
be**, because what a spell does when it resolves cannot change whether the pool
paid for it. What the fetch moves is the route: *Trinket Mage and a Lantern both
cast by turn 5* read **0.78%** when nothing tutored, which was the deck drawing
both halves naturally, and reads **7.76%** now, on the same seven groups and the
same 4,120,116 compositions. `decks/lantern-route-b.criteria.toml` is that
question on its own.

**The third route is now answered exactly, and it was overstated.** For a year
the file stood in for it with *Urza's Saga drawn by turn 3*, labelled an upper
bound, because chapter counters did not exist. The route itself — the Saga in
play by turn *t* and the Lantern still in the library on turn *t*+2, which is
when chapter III looks — reads **8.32%** on the play and **9.14%** on the draw,
against **9.09%** and **10.10%** for the stand-in, exactly, and checkable by
hand. `decks/lantern-route-c.criteria.toml` asks it with the chapter declared
as an effect and gets the same number to the last digit; a test holds the two
together.

**And the union of them now has a number.**
`decks/lantern.criteria.toml` is the whole question — four routes, because the
tutors that put the Lantern in your hand and the tutors that put it straight
onto the battlefield are priced differently — and it reads **46.64% ± 0.11** on
the play and **53.64% ± 0.11** on the draw. It read 48.09% and 55.01% until
[#61](https://github.com/cramt/progress-engine/issues/61): the engine played
Search for Azcanta — a `{1}{U}` enchantment whose land is a back face it
transforms into — as an untapped blue land drop, and 1.4 points of the answer
were spells cast off a blue source that cannot exist. It is estimated rather than
enumerated, at 12 groups and 659,902,464 compositions, and that is the honest
state of the question rather than a defect in the file: nine of its ten branches
price a cost, and pricing one splits the manabase before any card query splits
anything. The same file records what the mana costs the deck, by asking each
route twice: *a tutor that can find the Lantern, drawn by turn 5* is 45.23% and
*castable in time to matter* is 27.15%; for the two that tutor onto the
battlefield it is 21.09% against 4.98%, which is a factor of four. Those gaps
are what the file's old hand-written `{ turn = 4, query = "t:land", min = 3 }`
clauses were standing in for.

**Making the Saga exact did not move the union**, to six digits on the same
200,000 hands: 0.480885 before and after, measured before the fix below. The hands the stand-in overstated are
Sagas whose chapter III looks for a Lantern already drawn, and every one of
those is a hand route 1 already counts. So the union was never inflated by the
Saga; the Saga's own number was.

**So every route is priced, and the north star is answered as a floor, not
met.** The number still leaves out inputs that move it, all named in the file
and all in the same direction: the deck's seven mana rocks, which `can_cast`
does not count because only a land arrives without being cast; mulligans, which
are declarable now and which the file does not declare, because the keep rule is
the pilot's to state and not this tool's to guess; and the ten lands that may
enter untapped for 2 life, all assumed to enter tapped.
A deck that fails its 75% target by 27 points as a floor has not been shown to
pass it. The largest of those — mana rocks, on an artifact deck — is where the
next honest movement in this number is.

**Life from the Loam.** *How often is Loam in my graveyard by turn 5?* Needs the
graveyard to be a thing you can ask about, and needs a card routed into the yard
to count as arriving there — because for this deck that is the *good* route, not
a consolation prize.

**Answerable for a deck built on surveil lands. Not answerable for the actual
deck**, which was checked against its own list after this was first written down
as done. It contains no surveil or scry land at all. Its routes to the graveyard
are Loam's own Dredge 3, six mana-gated self-mill spells, and cycling lands and
discard outlets — and the `[[effect]]` grammar expresses none of them, because
every one is either mana-gated or a replaced draw rather than a land drop.

**That check is now the tool's rather than a reader's.** It was written into a
comment in `decks/loam.criteria.toml` because the committed index carried no
oracle tags, and on a tagless index `otag:surveil` matching nothing and nobody
having fetched that tag are the same empty result. The index carries all eight
tags now, so the standard library's two entries are evaluated against real
membership and match zero cards in that list. Same zero, different fact.

**One route out of five is expressible, and it is the one nobody had thought to
write down.** A sorcery goes to the graveyard when it resolves, so paying
`{1}{G}` for Life from the Loam puts Life from the Loam in the graveyard. Asked
that way — a clause for the card and a clause for `can_cast = "{1}{G}"` — the
deck reads **9.56%** on the play against **11.22%** for holding the card with no
question asked about the mana, and 87.81% for the mana with no question asked
about the card. It is a genuine joint and not a product, for the same reason the
Lantern's is: a hand holding the Loam has one fewer slot that could have been a
land. It is also a lower bound on the north star, because dredge, discard and
the self-mill package can only add to it.

So the machinery is real and the north star is not met. Recording it as met was
the same mistake as recording the Lantern question in its easy form: a feature
was built for the deck that was imagined rather than the one on the table.

**And the self-mill package is nearer than it looked.** Aftermath Analyst mills
three on resolution: nothing reaches the hand, the cards come off the top in
library order, and they are routed to a zone — which is `look` plus
`to_graveyard`, the mechanism the land-drop tier already runs, on a different
trigger. It is refused as a replacement draw, and for a pure mill it is not one
([#62](https://github.com/cramt/progress-engine/issues/62)).

Both needed the mana model, and that was not obvious until the real lists were
read. Lantern's three routes can now be written down together, and all three
are priced rather than assumed: a clause can ask whether the mana was there and
a clause can ask what it was spent on. Loam's self-mill is six spells and a
dredge trigger, every one of which costs mana or replaces a draw — the mana half
of that is now expressible and the replaced draw is not, which is the honest
state of it and is why the land-drop tier built for this deck still does not
fire on it once.

The general lesson, worth more than either deck: **the effect tiers were chosen
from decks nobody had opened.** Land drops were picked first because they are
free and bounded, which is true, and because surveil lands were assumed to be
how these decks filter, which was not. Almost everything real decks do costs
mana.

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

So the tool refuses. An empty library is refused rather than answered with 0%. A
query naming a keyword no card in the index has is refused by name. A query
naming an oracle tag this index never fetched is refused, because *nobody asked
about that tag* and *no card is in it* are the same empty result and very
different facts.

The one refusal that has since been traded away is `RunError::TooWide`, and what
it was traded for says what the principle is really about. A question too wide
to enumerate is now answered by sampling, because the intended caller is a
graphical builder that cannot pass a flag and cannot act on advice to ask
something smaller. What makes that acceptable is not that the answer is good
enough; it is that the answer cannot be mistaken for the other kind. The run
says, above the numbers, which of them it estimated rather than enumerated;
every sampled figure is quoted with its error bar and every enumerated one
without; the JSON says, per answer, which engine produced it and why.
`--exact` restores the refusal for anyone who would rather have no answer than
an approximate one. A silent fallback would have been a number quietly changing
kind, which is the same failure in a new costume; a labelled one is a different
answer to a question that was honestly too big.

**Which question was too big is now a question worth asking.** The fallback
used to be a property of the run, because the enumeration was one enumeration,
sized for the whole file as the join of what its hardest question needed. So a
criterion counting `cat:"Ramp"` was estimated because it shared a file with a
`can_cast` clause, and refusing — or sampling — an easy question on account of
a hard one beside it is not honest either. The file is now partitioned into
classes of questions that read the same things, each class is enumerated on the
narrowest grouping and the fewest checkpoints that can answer it, and only a
class that is still over the ceiling falls back. Nothing about the numbers
changes when it narrows: a coarser grouping is a marginal of the finer one,
draws between two turns nobody reads have the same joint distribution merged as
separate, and two lands a cost cannot tell apart are one source to the matching
it runs. That is what makes it a narrowing rather than an approximation, and all
three are asserted as properties rather than assumed.

**And a run says how it enumerated.** A file being several enumerations means
*how wide was this* has an answer per question rather than per run, and for a
while the only one that reached the output was the width of the class the run
refused — so the figures this project quotes about its own narrowings could not
be reproduced from a run that performed them. The `enumerations` block is that
closed: per class, what it reads, how many groups and compositions it cost, and
whether it was walked or sampled. Same argument as the provenance block, aimed
at the numbers about the numbers.

### Ask, don't guess

Where data is not derivable, it is fetched and dated rather than approximated.

Land cycles are the worked example. `is:shockland` and `otag:tapland` cannot be
derived from oracle text — "enters tapped unless you control two or fewer other
lands" is conditional in a way no regex survives. The old conclusion was that
they could not be supported. The right conclusion is to ask Scryfall and record
the date of the answer, which is the same standard the card index is already
held to.

It paid twice. Asking also turned up `otag:conditional-tapland`, which is the
179 lands whose tapped-ness is contingent or chosen — and `otag:tapland`
excludes every one of them, so the two tags together separate *this land enters
tapped* from *this land offers you a decision*. A model built on the one tag
would have had to fold those together and would have been wrong about every
shockland in either direction. Nobody could have derived that distinction; it
was there to be asked for.

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
**The land drop is the second resource it covers.** `[land_drop] prefer = [...]`
is the same declared priority over queries, ranking the lands you could play
rather than the cards you just looked at — which is what lets one file hold a
routing effect and a mana question, since both then read the drop the list
chose.
**The turn's mana is the third.** `[casting] prefer = [...]` ranks the spells
you would cast when the pool cannot pay for all of them — the same list, over a
resource that depletes rather than over a set you looked at.
**A tutor's target is the fourth.** `fetch = [...]` on an `[[effect]]` ranks
the cards it would go and get, over the whole library rather than over a set you
looked at, and the first one the library still holds is the one it takes.
**Mulligan bottoming is the fifth**, as `[mulligan] bottom = [...]`, beside a
keep rule written as count clauses and a floor that is kept whatever it holds.
It varies the mechanism in exactly one place, and the variation is forced: a
tie inside one entry is settled at random and priced, where every other list
settles it by the card the decklist names first. The other lists get away with
an order because what they choose is read by the questions that already tell
those cards apart; a mulligan decides which hand *every* question is of, so the
narrowest class merges cards one entry holds, and merging two cards an order
put in different places puts back a different card. A uniform choice commutes
with merging, and an order does not — the property test that holds narrowing to
the unnarrowed answer fails against the order and passes against the coin.

### Mana

Two lands is not two mana, and holding six Opts is not casting six Opts.

Mana appears twice, and the two are not the same problem.

As a **gate**, it decides whether a criterion is satisfiable at all: *can I cast
a `{1}{W}{U}` three-drop on turn three?* This is not answerable from counts a
user writes, because one Hallowed Fountain counts toward both `produces:w` and
`produces:u` and a conjunction of independent counts is satisfied by a hand that
cannot pay. It is a bipartite matching, and the engine can solve it exactly per
composition while the user cannot express it — so it has to be a primitive.

**The gate ships**, as `zone = "battlefield"` for lands and `can_cast` for a
cost. It cost nothing in enumeration width to count lands in play and, at first,
a great deal to price them: telling a Plains from an Island splits groups no
query splits, and on either real deck in `decks/` a castability question was a
dozen and a half groups — 1.2 billion compositions at **turn four**, against a
ceiling of five million.

Most of that price was not honest, and the parts that were not have been taken
out. The first was
that the split was charged to every other criterion in the same file, because
the grouping was decided once for the file; that is
[#31](https://github.com/cramt/progress-engine/issues/31), and the split is now
charged to the clause that asked for it. The second was that the split was
finer than the question: `Cost::payable` runs Hall's condition over the pip
kinds the cost demands and nothing else, so `{1}{U}` cannot tell a Plains from a
Forest and was paying to. Keying the manabase on the palette **intersected with
what the class's costs demand** is
[#55](https://github.com/cramt/progress-engine/issues/55): sixteen land profiles
become four, turn four becomes 41,250 compositions, and both north-star decks
answer `{1}{U}` exactly through turn six where they used to estimate it from
turn four.

What a narrowing has to survive before it ships is *everything the walk does*,
not only the clause that motivated it. Two lands with the same restricted
palette are interchangeable to the drop count, to what is in play, and to
whether a land arrived this turn, because all of those read sums over land
groups and a sum does not care how its terms were labelled. They are **not**
interchangeable to a declared land-drop priority, which plays the first land its
ranking reaches and ends that ranking in *the card your decklist names first* —
merging renumbers it and plays a different land. So a run that declares one
keeps the whole palette and pays for it, which is a real cost on a real deck and
is stated rather than quietly taken
([#56](https://github.com/cramt/progress-engine/issues/56)). A narrowing that
cannot be proved is not applied: a silently unsound one changes an answer it
cannot justify, which is strictly worse than the estimate it replaces.

As a **budget**, it is spent. An opening hand of one Island and six Opt casts
*one* Opt, because the first one consumes the Island. A model where an effect
fires whenever you hold the card overstates that turn sixfold, and the number
looks perfectly reasonable in a report.

**The spending ships**, as `cast`. A clause counts the spells a turn's lands
actually paid for, the bill of a line is added up and settled as one matching
rather than asked spell by spell, and a spell that is cast leaves the hand — so
the same copy cannot be cast twice and the count of what you are still holding
goes down. HANDS.md hands 1, 2 and 3 are that, worked and asserted: one Island
and six Opt casts one, a tapland casts none, and seven Opts and no land do
nothing at all, on a run that also reports six Opts in hand so the naive number
is visible beside the real one.

Which spell you cast when you cannot cast both is a **declared priority over
queries** — the same mechanism as mulligan bottoming and selection routing, gated
by a resource instead of by a looked-at set. Not a fourth policy language. It
differs from the land drop's list in exactly one way, and that difference is
stated in every run that uses one: **a spell the list does not name is not
cast**. A land nobody ranked is still played, because declining a drop is not
something a preference can be read as asking for and because "any other land"
costs one query bit; "any other spell" would make every card in the deck carry
its own mana cost into the grouping, which shatters a Commander library along a
line nobody asked about. So the list is *the line you are asking about*, which
is the same reading the gate already takes of the land drop.

**What it costs is every class, not one.** A cast spell leaves the hand, so a
criterion counting `cat:"Ramp"` beside a budget depends on what the pool paid
for three turns earlier — which depends on the manabase and on what each named
spell costs. A file that declares `[casting]` therefore prices the manabase on
every question in it and reads every turn rather than a total. A file that
declares none pays nothing, and on the 1,848 deck-criteria-index combinations
in this repository the budget moved no digit of any number.

**What it does not do is draw.** As soon as Opt draws a card, *cards seen by
turn T* stops being a fixed schedule and becomes path-dependent, which changes
the shape of the enumeration rather than the state carried along it — and that
is a measurement rather than a worry. Every card the walk might or might not
draw needs a checkpoint of its own, each checkpoint multiplies the enumeration
by the group count, and a turn with *T* mana can cast *T* cantrips. On the two
decks in `decks/` that puts the cheapest possible line over the ceiling at turn
five and anything with a colour in it over at turn three, against north stars
that both ask about turn five. A feature that is sampled on every question it
exists for is not a feature, so it is refused by name and the refusal carries
the numbers: [#57](https://github.com/cramt/progress-engine/issues/57), which
wants [#18](https://github.com/cramt/progress-engine/issues/18)'s
non-fixed population rather than more checkpoints. HANDS.md hand 5 is the hand
that stays open, and it is the only one that does.

**And a gate beside a budget asks what the line left.** One pool, one
accounting: in a file that declares `[casting]`, `can_cast` is answered against
what the declared line did *not* spend. Answering it against the whole turn's
lands would be two claimants on one resource — the mistake the land drop taught
us not to make — and a file that declares no casting priority spends nothing, so
nothing moves.

**Two decisions about the pilot's lands, and they are settled differently.** A
shockland's tapped-ness is a choice — `otag:conditional-tapland` says the card
offers it and settles nothing else — and the run assumes the pessimistic half
and names every card it assumed it about.

The other was a land drop being one resource with two claimants: the effect
library spends it choosing which land to look with, the gate spends it choosing
which land pays, and a file holding both was refused rather than arbitrated.
**It is now declared**, as `[land_drop] prefer = [...]` — the same declared
priority over queries as mulligan bottoming and selection routing, over a third
resource rather than a fourth policy language. The list is read in order, a land
it does not name is played last, a tie inside one entry goes to the deeper look
and then to the decklist's order, and **every run that resolved a drop this way
prints the list** beside the tapped-ness assumptions. Declaring nothing and
holding both claimants is still refused, because *which land would you have
played* is a question the pilot answers and the tool does not: what changed is
that the refusal now names the remedy instead of telling you to ask the two
halves in separate files.

The width behaves the way `can_cast` taught us to expect: a preference that
separates lands something already separated is free, and one that draws a new
line costs a group. In the case this exists for — a routing effect and a mana
question in one file — the natural list is free, because routing has already
split the land it routes with.

**What it is not free of is the colour narrowing**, and that is the one real
cost of declaring a priority. The ranking ends in *the card your decklist names
first*, so it reads the manabase in a way no cost does, and #55 cannot merge two
lands it ranks apart. On `decks/lantern.txt` that is 41,250 compositions without
a priority against 1.2 billion with one — an exact answer against an estimate,
at turn four. It is recoverable by merging only lands the ranking already places
side by side, which is
[#56](https://github.com/cramt/progress-engine/issues/56).

### The library is not a fixed population

`chip_stats::for_each_checkpoint_path(groups, gaps, f)` took the group sizes once
and computed what remained as `groups - drawn`, so the population was a constant
of the whole path by construction. A tutor needs it not to be: Trinket Mage does
not draw you a Lantern, it takes the Lantern out of the library and puts it in
your hand, and the library gets both smaller *and* differently composed.

**Half of that ships, and the half is the cheap one.** The issue this comes from
([#18](https://github.com/cramt/progress-engine/issues/18)) splits it, and the
split is the scope decision:

- A **tutor** is a *deterministic* removal from a named group. Given the
  checkpoints reached so far there is one answer, so it costs no branch: it is a
  subtraction from the pool the next gap is dealt out of, and the enumeration is
  the same width it always was. `decks/lantern-route-b.criteria.toml` is seven
  groups and 4,120,116 compositions at turn 5 with the tutor and without it, to
  the composition.
- **Exiling off the top** is itself a draw. The three cards Devourer of Destiny
  exiles are a random sample, so what remains is a distribution rather than a
  subtraction, and it branches the path the way a draw does. That is not built,
  and it is filed rather than approximated.

`chip-stats` stays Magic-free, which was the condition the issue set. What it
gained is a vocabulary word — **removals** — beside populations, groups and
draws, and its known-answer tests state it in exactly those terms: two groups of
two, draw one, remove one from group 0, draw one more, and the answer is 3/4
where the plain walk says 4/6. Nothing in the signature knows what a tutor is.

**The sampler follows, and the agreement is the acceptance test.** The two
engines shrink the library by completely different means — the exact one
subtracts from the pool the next gap is dealt out of, the sampler reaches into
the undealt tail of a shuffled deck and swaps the card past the end — so a fetch
is a new way for them to disagree, and they are asserted against each other on
both what ends up in hand and what is left behind.

**Whoever declares a tutor says what it fetches, and the run names it.** Same
discipline as `[land_drop]` and `[casting]`, and it is a declared priority over
queries rather than a fifth policy language. What it is allowed to do is
restricted rather than open: `to = "hand"` is a tutor, `to = "battlefield"` on a
land drop is a fetchland replacing itself, and a land arriving off a *spell* is
refused by name because whether it enters tapped is a fact about the spell that
fetched it and no tag separates Rampant Growth from Nature's Lore.

**And deck thinning finally has a number.** Does a fetchland meaningfully
improve your subsequent draws? On `decks/loam.txt`, where fetchlands are a large
part of why the deck functions, the answer is **yes, exactly, and negligibly**:
the largest figure `decks/loam-thinning.criteria.toml` moves is *two Loam Access
cards by turn 8*, from 58.9176% to 59.0695%, which is one game in 658. Four
fetchlands in 98 cards is 0.57 of one cracked by turn 8, each removing one land
from a library of about ninety. The mean lands *drawn* falls at the same time,
because the land the fetch found is already in play. It is a trade rather than a
free roll, and both sides of it are now printed instead of argued.

### Zones are the real question

"Did I find the card" is not a well-formed question. "Is the card in this zone by
this turn" is. Every number printed before zones existed silently meant *in
hand*, and for a graveyard deck that is the wrong question asked confidently.

A clause now names its zone, and silence still means `hand` so that no file
written before this moves. `graveyard` is askable, and reachable exactly when
some effect in the run routes a card there; in a run where none does it is
correctly empty and the run says so rather than letting that zero pass for a
measurement. `battlefield` is askable **for lands**, which is the half of it
that needs no casting: a land arrives on a land drop, one a turn, and the
enumeration already walks those. For anything that has to be cast it is still
refused by name — not because which spells you cast is unknown, which the
budget now answers, but because where a spell *goes* after it resolves is not
modelled at all. `cast` counts the castings and says so in those words, and a
Lantern counted on the battlefield would still be there after somebody blew it
up.

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
- A tag that cannot be fetched costs that tag and not the download, and an index
  carrying no tags at all says so when something asks for one
  ([#50](https://github.com/cramt/progress-engine/issues/50)) — shipped. The
  empty-vocabulary reasoning that is right for keywords is wrong for tags:
  keywords are derived from the cards, so an old index genuinely cannot know,
  while "this index fetched none" is a fact its own header states.
- A keyword the index's card pool does not carry is refused by name
  ([#53](https://github.com/cramt/progress-engine/issues/53)) — shipped. It was
  written here as the precedent for the tag refusal while having no caller at
  all: the check existed, was unit-tested, and nothing ran it.
- Mulligans are declared, as `[mulligan]` with a `keep` rule, a `bottom`
  priority and a `down_to` floor, and none of the three has a default
  ([#7](https://github.com/cramt/progress-engine/issues/7)) — shipped. It stays
  exact: under the London mulligan every depth is a fresh deal, so the answer is
  a sum of one enumeration per depth weighted by the chance of reaching it, and
  the sampler plays the same mulligans game by game as the check. A tie inside
  one `bottom` entry is priced rather than broken, for the reason
  [One mechanism for selection](#one-mechanism-for-selection) gives.
- **Turn 0 after a mulligan is the hand that was kept**: five cards after a
  mulligan to five. The cards that went back are on the bottom of the library,
  where a tutor still finds them and no draw in the horizon reaches them.
- **A run with a mulligan is judged on the mulligan's number, and prints the
  keep-your-seven number beside it**, labelled, in the text and the JSON. A run
  with none keeps every seven, exactly as before, and says so above its numbers:
  a default that moved no number in this repository is kept, and a default that
  goes unmentioned is not.
- The effect library ships, autoloads as a prelude, and is keyed on queries
  ([#43](https://github.com/cramt/progress-engine/issues/43)) — shipped.
- Overlapping effects resolve last-wins, per card — shipped.
- Selection is a router over a declared priority, and the land-drop tier ships
  first because one drop a turn bounds it
  ([#17](https://github.com/cramt/progress-engine/issues/17)) — shipped. The
  mana-gated tier is refused by name until
  [#10](https://github.com/cramt/progress-engine/issues/10).
- Which land you play when you could play either is a declared priority over
  queries, `[land_drop] prefer = [...]`
  ([#54](https://github.com/cramt/progress-engine/issues/54)) — shipped, and it
  is the second resource on the one mechanism rather than a third language. The
  list is total: an unnamed land is played last, and a tie inside one entry goes
  to the deeper look and then to the decklist's order. A run that used it prints
  it. Holding a routing effect and a mana question with no list declared is
  still refused — the remedy is named, and the tool does not pick for you.
- The standard library declares what a card looks at and never where the cards
  go, because the destination is part of the question.
- A mana model is in scope, staged gate-first then budget
  ([#10](https://github.com/cramt/progress-engine/issues/10)). **Both halves
  ship, minus the draw**: lands in play, tapped-ness and `can_cast` as a
  matching over the lands a composition put down; then `cast`, which spends
  those lands on a declared line and counts what they paid for. What a cast
  spell then *does* — Opt's draw — is refused by name and measured rather than
  sampled ([#57](https://github.com/cramt/progress-engine/issues/57)).
- Which spell you cast when the pool cannot pay for both is a declared priority
  over queries, `[casting] prefer = [...]`, and a spell the list does not name
  is not cast at all. That is the one way it differs from the land drop's list,
  it is the difference between a line and a preference, and every run that used
  one prints it.
- The library is a population that can shrink without being drawn from
  ([#18](https://github.com/cramt/progress-engine/issues/18)) — **the tutor half
  ships**. `fetch = [...]` on an `[[effect]]` is the fourth declared priority
  over queries, `to` says where the card is put, and both are printed by every
  run that used one. It is a deterministic removal, so it costs no enumeration
  width: Route B on `decks/lantern.txt` is 4,120,116 compositions at turn 5 with
  the fetch and without it. Exiling off the top is the other half, it is a draw
  rather than a subtraction, and it is filed rather than estimated.
- `on = "cast"` fires, and what it may do is a `fetch` rather than a `look`. The
  budget knows which spells a turn paid for; a replacement draw is still refused
  by name and still measured ([#57](https://github.com/cramt/progress-engine/issues/57)).
- A fetched land is counted where it is and not tapped for. `otag:fetchland` is
  54 cards and holds both Scalding Tarn, which fetches untapped, and
  Terramorphic Expanse, which does not — so what left the library is answered
  exactly and what the land makes is refused rather than guessed in whichever
  direction flatters.
- A land whose tapped-ness the pilot chooses is assumed to enter tapped, and
  every run that depended on the assumption names the cards it made it about.
  Understating a shockland manabase is the failure this project would rather
  have: a number a deck can beat is better than one it cannot reach. It is a
  stated assumption rather than a hidden one, and it is meant to become
  declarable.
- Castability is refused where the card data cannot price it — an index with no
  `produces`, or without both tapland tags — rather than answered as zero or as
  untapped, which are the two confident wrong numbers available.
- A criterion can hold `any_of`, one level of alternation over branches of
  clauses, and its answer is the union of the branches rather than their sum
  ([#49](https://github.com/cramt/progress-engine/issues/49)) — shipped. The
  routes overlap by design, so a sum would exceed 1; the union falls out of
  each enumerated path contributing its probability once, in the same walk, so
  the disjunction is exact and adds no groups beyond the queries its branches
  already name.
- The enumeration is sized per class of question rather than per file
  ([#31](https://github.com/cramt/progress-engine/issues/31)) — shipped. A
  class keeps only the queries it reads, only the turns it names, and what a
  land makes only where something asks whether a cost could be paid; everything
  else merges. A question over the ceiling on its own is still estimated, and
  now it is the only one. `decks/lantern.criteria.toml` is the case that shows
  what that is worth once a file asks its real question: its north star is a
  union of four routes, nine of whose ten branches price a cost, and at 12
  groups and 659,902,464 compositions no narrowing brings it under the ceiling.
  The other twelve questions in that file are exact, at between 10 and 122,880
  compositions each. Sized per file they would all have been estimates.
  `decks/loam.criteria.toml` is the other half of the same claim: 8 groups and
  14,057,472 as one enumeration, which is sampled, against a widest class of 6
  groups and 1,026,432, which is not.
- A cost is enumerated on the colours it demands and no others
  ([#55](https://github.com/cramt/progress-engine/issues/55)) — shipped. A
  Plains, a Swamp and a Forest are one source to `{1}{U}`, so both decks in
  `decks/` answer that clause on five groups and 41,250 compositions at turn
  four rather than on seventeen and 1.2 billion — exact where it was an
  estimate, through turn six. It is applied only where nothing but the cost
  chooses which land was played: a declared `[land_drop]` priority ranks lands a
  cost cannot tell apart, so merging them would play a different land, and there
  the whole palette is kept and paid for
  ([#56](https://github.com/cramt/progress-engine/issues/56)).
- Every run reports how it enumerated: per class of question, what it reads, how
  many groups and compositions it cost, and whether it was walked or sampled.
  The width figures in README and here are read off that block rather than
  measured beside it.
- A criterion names the zone it asks about, silence means `hand`, and
  `battlefield` was refused until castability existed rather than approximated
  ([#40](https://github.com/cramt/progress-engine/issues/40)) — shipped, and the
  refusal has since narrowed to the part that is still true: lands are counted
  there, and a permanent that has to be cast is not.

## Not yet decided

- Whether a user can *disable* a stdlib effect rather than override it.
- Which mulligan the north-star files declare. The feature ships; the keep rule
  is the pilot's, and neither file states one yet, so both still answer on a
  kept seven and say so.
- Choosing the mulligan rather than declaring it — the best keep threshold and
  the best cards to put back for a weighted set of criteria
  ([#63](https://github.com/cramt/progress-engine/issues/63)).
- Whether the pod matters at all now that the tool is format-agnostic
  ([#22](https://github.com/cramt/progress-engine/issues/22),
  [#23](https://github.com/cramt/progress-engine/issues/23)).
