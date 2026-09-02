# progress-engine

Draw-probability tests for Magic: The Gathering decklists.

Named for Jin-Gitaxias's faction of New Phyrexia, on the grounds that obsessively
recalculating whether your deck is perfect yet is blue-aligned behaviour.

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

```js
criterion("t1 dork into t2 commander", (t) =>
  t(1).count('t:land') >= 1 &&
  t(1).count('cat:"Mana Dork"') >= 1 &&
  t(2).count('t:land') >= 2
, { atLeast: 0.55 });
```

That is a unit test for a deck. Change a card, run it again, see which assertions moved.

## How it works

Probabilities are **exact**, not simulated. Cards are grouped by which of your queries they
match, and the tool enumerates count-vectors over those groups, weighting each by its
multivariate hypergeometric probability. There is no shuffler, so there is no sampler bias to
chase — and it is fast enough that the answer is instant.

The JavaScript API deliberately exposes only `count(query)` rather than the cards themselves.
That constraint is what keeps the engine exact: a criterion is a pure function of how many
cards of each query you drew, so it is evaluated once per possible composition instead of once
per simulated hand.

Which queries a file uses and how far into the game it looks are both discovered by running
it, so neither is declared up front. When a criterion reaches for a query or a turn the run
was not set up for, that run is discarded and repeated against one that covers it — because
`a && b` never evaluates `b` while `a` is false, and a turn nobody modelled would otherwise
answer zero for every hand.

`--simulate` is not a second feature set — it is a second implementation. It shuffles and
deals where the default enumerates, and wherever both can answer they must agree, which is
asserted at three levels: unit, through the JavaScript bindings, and end to end through the
binary. Two independent routes to the same number is how a mistake in either one gets caught
rather than believed, and that is reason enough for the crate to exist. It is also the escape
hatch for questions too wide to enumerate: compositions multiply with the number of groups the
queries split the library into and with how deep the turns go, so a criterion joining seven
category queries at turn six is refused rather than answered in an hour:

```
$ progress-engine test simple-ramp.txt wide.criteria.js
Error: this question is too wide to answer exactly: 6158592 compositions across 6 groups.
Reduce the number of distinct queries, or ask about an earlier turn.
```

Six groups from a file that asks seven questions, because the refusal lands part-way through
discovery. Queries are learned by running the file, so they arrive a few at a time, and five of
them — six groups, counting the cards none of them matched — already put the estimate over the
ceiling. The last two are never reached, so the figure names what the run had learned when it
gave up rather than what the file would eventually have asked for. Sampling does not care how
many groups there are, so it answers that question approximately instead of not at all.

What `--simulate` is *not* is a way to inspect individual cards: both modes hand a criterion
the same `count(query)` and nothing else, so anything that turns on *which* cards — a specific
interaction, an ordering, the best card in hand — cannot be written in either. That API is
wanted and does not exist, which is
[issue #11](https://github.com/cramt/progress-engine/issues/11). Describing it here as though
it shipped would be this tool's own defining failure mode aimed at its documentation: a
confident claim about something that is not there.

## Status

Working today:

| Command | What it does |
|---|---|
| `progress-engine parse <deck>` | The canonical Archidekt decklist parser, as JSON |
| `progress-engine test <deck> <criteria.js>` | Evaluate criteria and report PASS/FAIL |

```
$ progress-engine test simple-ramp.txt simple-ramp.criteria.js
PASS keepable opener (2-5 lands)   78.97%  (needs 70.0%)
PASS turn-1 accelerant             51.04%  (needs 35.0%)
PASS commander on turn 2           44.29%  (needs 30.0%)
     any ramp by turn 3            94.39%

PASS: 3 of 3 assertions met
```

JSON goes to stdout, the verdict to stderr, and the exit code reflects it — so a
caller piping stdout through `jq` cannot lose the failure.

That list is also checked against the rules of the format before any of it is
computed, and this one is silent because it is legal. When it is not, the run
says so without refusing to answer: see
[decks you cannot legally play](#decks-you-cannot-legally-play).

A criterion with no `atLeast` is informational: it reports a number and cannot
fail. Add `--draw` to model being on the draw, and `--simulate` to sample
instead of enumerate (slower, approximate, and reported with standard errors).

The JSON also carries a `provenance` block — the tool's version, the date the
card index was built, and SHA-256 hashes of the decklist and criteria files as
they were read. A percentage that moved since last week is useless on its own,
because your deck, your criteria, the index and the tool itself each move it and
in the output they are indistinguishable: a number changed. Naming the inputs is
what lets a comparison say *which* one changed rather than only that something
did. An index that never recorded when it was built reports `null` there, for the
same reason a card with no legality word comes back unknown: a plausible date
nobody can vouch for is worse than an admitted gap.

### The card index it needs, and does not build

Only one of those two rows works from a fresh clone. `parse` reads the decklist
text and nothing else, so it needs no card data and never has. `test` has to
know what a card *is*, and it gets that from a Scryfall index this repository
does not produce.

It looks for `index.json` under `$SCRYFALL_CACHE`, failing that
`$XDG_CACHE_HOME/scryfall`, failing that `~/.cache/scryfall`; `--index <path>`
points it somewhere else entirely. The tool that writes that file is a separate
`scryfall sync` shell tool that does not live here, so a clone of this repo
alone gets:

```
$ progress-engine test simple-ramp.txt simple-ramp.criteria.js
Error: no Scryfall index at /home/you/.cache/scryfall/index.json.
Build one with: scryfall sync
```

Loading the library is stricter still: a card the index does not know stops the
run with *run `scryfall check` first*, rather than being skipped, because a card
that cannot be looked up has no type line and would quietly skew every
probability it touches — the same reasoning that makes an excluded card announce
itself.

The split was deliberate: bulk fetching, rate limits and caching were already
solved in the shell tool, and two things that could disagree about what a card
is would be worse than one thing that lives elsewhere. It is still half a tool
presented as a whole one, and being told to run a command you do not have is
precisely the sort of confident wrongness the rest of this document is about.
Owning it — `progress-engine sync`, building the index here and carrying the
fields queries actually need — is
[issue #2](https://github.com/cramt/progress-engine/issues/2).

### Queries that match nothing

The defining failure mode this tool exists to prevent is a query that is
perfectly valid and matches no cards. It cannot be a parse error, and it yields
a confident 0% rather than a complaint. So every run reports what each query
matched, and says so loudly when that is zero:

```
$ progress-engine test simple-ramp.txt typo.criteria.js
note: query "cat:\"Rmap\"" matched no cards in this deck
FAIL misspelled category   0.00%  (needs 30.0%)
```

Unsupported *syntax*, by contrast, is refused outright — `power>=3` names itself
as an error rather than quietly matching nothing.

### Cards that are never in your library

Sticker sheets, attractions, planes, phenomena, schemes, vanguards, conspiracies,
dungeons and emblems live in a deck of their own or in no deck at all. Counting
them inflates the library and moves every probability with it: a 99-card list
with ten attractions answers questions about a 109-card library that does not
exist. They are left out of the library, and never quietly:

```
$ progress-engine test unfinity.txt criteria.js
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

### Decks you cannot legally play

Impeccable statistics about a deck nobody will let you sit down with are the
most embarrassing kind of confidently wrong number, and by the time the first
probability is computed everything needed to catch that is already in memory:
the quantities, the colour identities, the banlist word and which lines were
nominated as commanders. So `test` checks the list against the Commander rules
and says what it finds:

```
$ progress-engine test illegal-commander.txt criteria.js
WARNING: this is not a legal Commander deck
  color_identity: Assassin's Trophy has colour identity BG, which is outside G — the identity of Marwyn, the Nurturer
  singleton: 2x Sylvan Library — Commander is singleton outside basic lands and the cards that say otherwise
  singleton: 3x Sol Ring — Commander is singleton outside basic lands and the cards that say otherwise
  format_legality: Mana Crypt is banned in Commander
  format_legality: Sword of Dungeons & Dragons is not legal in Commander
  deck_size: a Commander deck is 100 cards; this list has 98 (97 in the library plus 1 in the command zone)
The numbers below describe the list exactly as written.
PASS two lands by turn 2  100.00%  (needs 30.0%)

PASS: 1 of 1 assertions met
WARNING: 6 legality problems above — this is not a legal Commander deck
```

Five rules: colour identity against the *combined* identity of the nominated
commanders, so partners work; the singleton limit; the banlist and everything
else the format does not admit; deck size; and whether what you nominated can
command at all. The same list is in the JSON as `legality`, each entry carrying
a `rule` token to match on, the `card` at fault and the sentence a human reads —
so a CI caller that wants an illegal list to be a build failure has everything it
needs to make that decision itself.

**A warning never fails the run.** Somebody brewing wants the numbers *before*
the deck is legal — a half-built list is the normal case, not the error case,
and a tool that refuses to answer until the ninety-ninth card is chosen is a
tool nobody reaches for at the point they most want it. But a clean `PASS`
sitting alone underneath a deck that cannot be played is precisely the report
this project exists to prevent, so the warning is said twice: in full above the
results, where it survives a run that then dies on an empty library or a
question too wide to enumerate, and again as a count beside the verdict, which
is the line a reader actually stops on. The exit code stays what it has always
been, a statement about your criteria and nothing else.

**A missing field is not a violation.** The card index is a cache built by a
separate tool, older copies of it predate half these fields, and the hand-written
test fixtures in this repository carry almost none of them. So the card-level
answers are `Option<bool>` rather than `bool`, and every rule below reads a
`None` as *no complaint at all* rather than as *illegal*. A checker that reads
absent data as guilt buries its one real finding under ninety-eight imaginary
ones and teaches its reader to skip the whole block — which is this project's
own defining failure wearing a checker's clothes. The test suite keeps an index
with those fields stripped out, and asserts that a card at two copies draws no
complaint when nothing ever said whether it could be repeated.

The colour identity rule is dropped altogether for a colourless commander, by
the same mechanism and for the same reason. An identity is stored as a list of
colour letters, so a commander that is genuinely colourless — Karn, Kozilek —
and a commander whose index entry never carried the field at all both arrive as
an empty list, and nothing can tell the two apart. Acting on that empty list
would mean reporting every coloured card in the deck as illegal on the strength
of a field that may simply not be there. So the rule is skipped for those decks:
it gives up a genuine check for the handful of commanders it applies to, and in
exchange no out-of-date index can produce ninety-nine invented violations. If
the index ever learns to say *absent* and *colourless* differently, that is the
day to put it back.

**An index entry that is not the card tells you nothing about the card.**
Llanowar Elves is legal in Commander, and against a real index this checker
briefly said it was not. The index is keyed by lowercased card name, Scryfall
prints tokens that share a name with a real card, and the token can win the key
— at which point every field belongs to the token: mana value 0, no colour
identity, `not_legal` in every format. Forty-one real cards are shadowed that
way today, Mutavault and Meteorite and Storm Crow among them. Reporting the
token's fields as the card's would be the same confidently wrong claim as
reading a missing field as guilt, so an entry whose type line says `Token` is
treated as no data by every rule that consults card data and draws no complaint
of any kind. Only deck size still applies to it, because counting lines in a
decklist needs no card data at all. This repository cannot repair the underlying
defect — once two objects share a key the information is gone, and the index is
built elsewhere — so it is filed as
[issue #38](https://github.com/cramt/progress-engine/issues/38) and declined
rather than laundered.

**There is no `--format` flag, so the format is inferred, and only ever from
evidence.** A list that nominates a commander is a Commander list and gets all
five rules. A list that nominates none gets none of them, and that is a refusal
rather than an oversight: four copies of a card are a violation in Commander and
correct in constructed, sixty cards is the reverse, and colour identity is not a
rule outside Commander at all. Nothing in an Archidekt export distinguishes a
sixty-card deck from a Commander list whose export lost its `[Commander]`
bracket, so guessing would mean being confidently wrong about every card in the
list rather than about one. It says nothing instead.

**What it does not check: the bounded counts.** Scryfall's data models the
*unlimited* rule — the ten cards, Relentless Rats and Shadowborn Apostle and
their friends, that say a deck can have any number of them — and that is the
rule enforced here. It does not model the *bounded* rule. Nazgûl says "up to
nine" and Seven Dwarves says "up to seven", no field in the index carries that
bound, and so **a deck with ten Nazgûl is illegal and this checker will not say
so.** The alternative was reading the number out of the oracle text, a
derivation that would be confidently wrong in both directions — silently
permitting what it failed to parse and silently forbidding what it misparsed —
which is a worse thing to ship than an admitted gap. Answering a narrower
question than your reader believes you answered is the failure this whole
repository is a reaction to, so it is written down here rather than left to be
discovered at a table.

### Questions the deck cannot answer

Two more routes to a confident number about nothing, both refused rather than
answered:

```
$ progress-engine test all-commander.txt criteria.js
Error: the library is empty: every card in the list is a commander or outside the deck

$ progress-engine test two-card-deck.txt criteria.js
Error: this question draws 7 cards from a library of 2
```

The second is refused identically under `--simulate`. Left to themselves the two
engines disagree by the whole answer: enumeration finds no dealable hand and
reports 0%, while sampling deals what it can and reports whatever that gives.

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
writes the JSON contract above and reads the card index, and `figue` parses
argv. One `#[derive(Facet)]` per type feeds all of them.

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
| `pe-scryfall` | Card data and Scryfall search syntax | Cards. No decklists. |
| `pe-criteria` | Grouping cards by query, evaluating exactly | Counts. Neither cards nor JavaScript. |
| `pe-js` | The JavaScript runtime and its bindings | V8, and the criteria contract. |
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

Legality splits along the same line and for the same reason. Whether a card is
banned, whether it may be repeated at all, whether its type line puts it in the
command zone and whether its identity fits inside a given one are all facts one
card settles alone, so they live in `pe-scryfall::legality`. How many copies the
list actually has, which lines were nominated as commanders and how many cards
there are altogether are all decklist facts, so the rules that need them live in
`pe-cli`. Splitting it that way is what lets the card half be tested exhaustively
against hand-written index JSON — including the missing-field cases that matter
most — without either crate learning what the other is.

`pe-stats` deliberately has no idea what a card is. Its tests are pure
known-answer arithmetic, so a failure there is unambiguously a maths bug rather
than a card-data bug. The same reasoning puts the evaluator behind a trait in
`pe-criteria`: the enumeration is tested with plain Rust closures, so a failure
there is an engine bug and a failure in `pe-js` is a bindings bug. Keeping those
distinguishable is worth the indirection.

`pe-sim` exists to check `pe-stats`, not to replace it. Where both can answer,
they must agree — and that agreement is asserted at three levels: unit, through
the JavaScript bindings, and end to end through the binary.
