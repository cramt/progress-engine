# Ichormoon Gauntlet

A draw-probability engine for Magic decks: a decklist and a criteria file in, how often the deck does each thing by turn N out, exactly by enumeration. [VISION.md](../../VISION.md) is the authority on what it is and refuses to be; this file only pins the words. Card data, decklists and the draw arithmetic belong to [Reality Chip](../reality-chip/CONTEXT.md).

## Asking

**Criteria file**:
A TOML document of questions and declared priorities about one deck. It is data, not a program, and an API for a graphical builder rather than a user interface.
_Avoid_: gauntlet, script, config

**Question**:
Anything a criteria file asks: a criterion or an expectation.

**Criterion**:
A named question answered with a probability, optionally judged against a bound.
_Avoid_: assertion, test

**Informational criterion**:
A criterion with no bound; it reports a number and cannot fail.

**Bound**:
`at_least` or `at_most`, the share of hands a criterion must reach or stay under. Inclusive.
_Avoid_: threshold, floor, ceiling

**Expectation**:
A named count answered with a mean and a distribution, never judged.
_Avoid_: distribution, "how many" question

**Clause**:
One requirement inside a criterion, read at one turn: a count of cards matching a query in a zone, a count of castings, or a gate.
_Avoid_: requirement, condition

**Branch**:
One alternative inside `any_of`, itself a conjunction of clauses. A criterion's answer is the union of its branches, never their sum.
_Avoid_: route (in the file format), alternative

**Turn**:
Cumulative and counted from 0: turn 0 is the kept hand, and "by turn N" means everything seen up to and including turn N. Everything happens at sorcery speed on your own turn.

**Seat**:
Whether turn 1 draws: on the play or on the draw.
_Avoid_: scenario

**Zone**:
Where a clause counts cards: hand (when a clause is silent), library, graveyard or battlefield. Casting is not a zone.

**Reachable**:
Whether anything in this run can put a card in a zone at all. An unreachable zone is correctly empty, and the run says so rather than passing its zero off as a measurement.

## Answering

**Exact engine**:
The enumeration that produces every number it can, with no shuffling.
_Avoid_: calculator, walker

**Sampler**:
The second implementation, which deals shuffled hands. It exists as the oracle the exact engine is checked against, and answers a question only when that question is too wide.
_Avoid_: simulator, Monte Carlo engine

**Composition**:
One way the draws can fall: how many of each group were seen by each turn read, with its exact probability. The unit that enumeration width is counted in.
_Avoid_: path (Reality Chip's word for the same thing), outcome

**Prepared run**:
A criteria file made ready to answer against one deck: every query checked against the index, every declared priority resolved, every refusal that can be made before a hand is enumerated made, and the questions partitioned into classes. A run that cannot be prepared is refused, with the notes that came before the refusal.
_Avoid_: setup, plan (the engine's count of questions)

**Class**:
A set of questions that read the same queries, turns and mana detail, and so share one enumeration.
_Avoid_: bucket, partition

**Narrowing**:
Coarsening a class's grouping, turns or colours without changing any answer. A narrowing that cannot be proven is not applied.
_Avoid_: optimisation, approximation

**Width**:
What an enumeration costs, in groups and compositions.

**Ceiling**:
The most compositions one enumeration may walk (five million). A class over it is too wide.
_Avoid_: limit, MAX_PATHS (outside code)

**Estimate**:
An answer the sampler produced because its class was too wide. Always labelled, always quoted with an error bar.
_Avoid_: approximation, simulation

**Inconclusive**:
An estimate within two standard errors of its bound.

**Refusal**:
Declining to answer, naming the query, key or zone that could not be modelled, rather than printing a confident wrong number.
_Avoid_: error (when it is deliberate), unsupported

**Confident wrong number**:
A percentage that looks exactly like a real one and reflects something unmodelled. The failure the tool exists to prevent.
_Avoid_: bug, inaccuracy

**Lower bound**:
An answer known to understate the truth because of a named input it leaves out, such as mana rocks or lands assumed tapped.
_Avoid_: floor

**Provenance**:
What a run records so that a moved number can be traced to its cause: the tool version, the index date, and hashes of the deck, the criteria file and the standard effect library.

## The pilot's decisions

**Pilot**:
The person playing the deck, who owns every decision the tool will not guess.
_Avoid_: user (when the decision is about play), player

**Declared priority**:
An ordered list of queries the pilot declares, whose first match is the choice. It is the one mechanism for every pilot decision: which land to play, which spells to cast, what a tutor finds, where a look routes cards, what to bottom after a mulligan.
_Avoid_: policy language, strategy, heuristic, ordinals ("the fourth resource")

**Entry**:
One query in a declared priority.
_Avoid_: tier, rank

**Land drop**:
The one land a turn puts onto the battlefield without casting it, chosen by `[land_drop] prefer` when more than one could be played.

**Line**:
The spells a turn casts, in the order the pilot declared them with `[casting] prefer`. A spell the line does not name is not cast.
_Avoid_: line (for anything but casting), sequence, play pattern

**Route**:
One way a deck reaches an outcome, differing in turn, zone or what it needs. Written as a branch.
_Avoid_: tier, path

**Mulligan**:
A London mulligan, declared as a keep rule, a bottom priority and the smallest hand kept whatever it holds (`down_to`).

**Depth**:
How many mulligans were taken.

**Opener**:
The seven cards dealt at one depth, before bottoming.
_Avoid_: opening hand (ambiguous with the kept hand)

**Kept hand**:
The hand after bottoming: turn 0.

**Keep-seven number**:
What a criterion would read had every first seven been kept, printed beside the mulligan's number.

**Strategy**:
The keep rule a run is played under: the pilot's, or one chosen for a weighted objective.

**Objective**:
The weights over criteria that a chosen strategy maximises. The weights are the file's, because "best" is taste.

## Effects

**Effect**:
A declaration of what cards matching a query do when something triggers them: look, route, fetch, or wait.
_Avoid_: ability, card script

**Standard effect library**:
The effects that ship with the tool and load before a file's own. They declare what a card looks at, never where the cards go.
_Avoid_: stdlib, prelude, standard library

**Trigger**:
What fires an effect: a land drop or a cast.

**Look**:
Examining cards off the top of the library. A look that routes nothing changes nothing.

**Routing**:
Sending looked-at cards to a destination the question declared. A router, not a filter: for a graveyard deck the discard pile is the goal.
_Avoid_: filtering, discarding

**Destination**:
Where a routed or fetched card goes.

**Tutor**:
An effect that takes a named card out of the library and puts it in hand or onto the battlefield. It shrinks the library without drawing from it.
_Avoid_: search, fetch (as a noun for the effect)

**Delayed effect**:
An effect that resolves a fixed number of turns after its trigger, after that turn's draw.

**Live effect**:
An effect that can move a card, which forces the run to read every turn rather than totals.

**Effect tier**:
A family of effects grouped by what pays for them: the land-drop tier (free, one a turn), the mana-gated tier, the replacement-draw tier.
_Avoid_: tier (unqualified)

**Replacement draw**:
Any card drawn because a spell or effect said so, Opt's plain "draw a card" included. Broader than the rules term. Refused.

**Deck thinning**:
A tutor or fetchland shrinking the library, and the change that makes to later draws.

## Mana

**Gate**:
Whether the lands in play by a turn could pay a cost, settled as a matching of sources to pips rather than by counting. Asked with `can_cast`.
_Avoid_: mana check, castability (as a noun for the clause)

**Budget**:
A turn's lands spent on the line. A spell cast leaves the hand, and each land pays once.

**Pool**:
The mana one turn's lands make. Where a line is declared, a gate asks what the line left.

**Bill**:
A line's summed cost, settled as one matching.

**Mana source**:
A land that makes mana. Mana rocks and creatures are never counted.
_Avoid_: mana source for a spell that spends

**Pip**:
One coloured or colourless symbol a cost demands. Generic mana is not a pip.

**Palette**:
The pip kinds a source can make.

**Land profile**:
A distinct combination of palette and tapped-ness among a deck's lands.
_Avoid_: mana profile

**Tapland**:
A land Scryfall tags as always entering tapped.

**Conditional tapland**:
A land whose tapped-ness is the pilot's choice or a condition (shocklands, MDFC backs). Assumed tapped, and named in every run that assumed it.
