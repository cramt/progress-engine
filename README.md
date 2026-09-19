# progress-engine

Draw-probability tests for Magic: The Gathering decklists.

Named for Jin-Gitaxias's faction of New Phyrexia, on the grounds that obsessively
recalculating whether your deck is perfect yet is blue-aligned behaviour.

This README is what the tool does today. [VISION.md](VISION.md) is what it is
for, what it refuses to become, and which of those two lists a given decision
came from.

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
chase — and it is fast enough that the answer is instant. Enumerating rather
than sampling also means a count's whole *distribution* falls out of the same
walk instead of having to be estimated: see [how many, not just how
often](#how-many-not-just-how-often).

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
| `progress-engine sync` | Build the card index from Scryfall bulk data |
| `progress-engine parse <deck>` | The canonical Archidekt decklist parser, as JSON |
| `progress-engine test <deck> <criteria.js>` | Evaluate criteria and report PASS/FAIL |

```
$ progress-engine test simple-ramp.txt simple-ramp.criteria.js
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

That list is also checked against the rules of the format before any of it is
computed, and this one is silent because it is legal. When it is not, the run
says so without refusing to answer: see
[decks you cannot legally play](#decks-you-cannot-legally-play).

A criterion with no `atLeast` is informational: it reports a number and cannot
fail. The two blocks with means under them are `expect` rather than `criterion`,
and they are the subject of [how many, not just how
often](#how-many-not-just-how-often). Add `--draw` to model being on the draw,
and `--simulate` to sample instead of enumerate (slower, approximate, and
reported with standard errors).

The JSON also carries a `provenance` block — the tool's version, the date the
card index was built, and SHA-256 hashes of the decklist and criteria files as
they were read. A percentage that moved since last week is useless on its own,
because your deck, your criteria, the index and the tool itself each move it and
in the output they are indistinguishable: a number changed. Naming the inputs is
what lets a comparison say *which* one changed rather than only that something
did. An index that never recorded when it was built reports `null` there, for the
same reason a card with no legality word comes back unknown: a plausible date
nobody can vouch for is worse than an admitted gap.

### How many, not just how often

A criterion answers *how often*, and for a long time that was the only question
this tool could be asked. But half of what anyone actually wants to know about a
decklist is *how many* — expected lands in an opening hand, ramp pieces by turn
three, mana available on turn four — and the only way to get at it was to write
five criteria with five different thresholds and difference them by hand:

```js
criterion("0 lands", (t) => t(0).count('t:land') === 0);
criterion("1 land",  (t) => t(0).count('t:land') === 1);
criterion("2 lands", (t) => t(0).count('t:land') === 2);
```

That is a histogram reconstructed by its reader, in their head, from a column of
percentages that do not say they belong together. It is arithmetic performed
somewhere nothing checks it, which is the same category of mistake as a
definition kept in your head rather than written down.

So there is a second kind of registration. `expect` returns a count rather than
a bool, and is answered with a mean and the whole distribution behind it:

```js
expect("lands in opener", (t) => t(0).count('t:land'));
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

**A value has to be a whole number, and there is a ceiling on it.** The answer is
a histogram, so every value it accepts needs a bucket of its own; `count()`
returns whole cards and is the only thing a criteria file may look at, so a
count, or a sum of counts, is representable by construction. Anything else is
refused by name rather than coerced:

```
$ progress-engine test simple-ramp.txt rate.criteria.js
Error: evaluating criteria: expect("lands per card"): 0.14285714285714285 is not a countable value: it must be a whole number from 0 to 1024.
An expectation is reported as a distribution with one bucket per value, and there is nowhere to put this one.
```

Dividing by seven to report a rate is a reasonable thing to want and this tool
does not do it. The alternatives were rounding 0.57 to 1, or picking a bucket
width nobody asked for and reporting a distribution over it — both of which
answer a different question than the one written down and leave no mark in the
output saying so. An admitted refusal beats a plausible histogram.

The same refusal, in the other direction, guards the two kinds against each
other. JavaScript will coerce a number to a bool and a bool to a number without
complaint, so a criterion that forgot its comparison would silently become "at
least one land" under a name promising a count, and an expectation returning
`true` would report a probability in a column headed *mean*. Both are errors
that name the offending registration and say which of the two you probably
wanted.

**A wide distribution is windowed, and says what it left out.** Lands seen by
turn twenty runs from zero to twenty-six, and printing twenty-seven buckets is
not a histogram but a wall. The human output shows the twelve contiguous buckets
holding the most mass — contiguous, because a histogram with holes punched in it
reads as missing data — drops ends that would print as `0.0%`, and states the
remainder rather than dropping it:

```
$ progress-engine test simple-ramp.txt lands-by-turn.criteria.js
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
an oversight.** `atLeast` on a criterion is a threshold on a probability: a
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
called `atLeast`.

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
either way. Of what is left, about 0.10s is starting V8 and answering the
question, and about 0.12s is still proportional to the index: reading it and
finding its lines. `sync` gets the same deal — deciding whether the index it
already has is Scryfall's latest reads one line instead of the whole file.

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
$ progress-engine test simple-ramp.txt typo.criteria.js
note: query "cat:\"Rmap\"" matched no cards in this deck
FAIL misspelled category   0.00%  (needs 30.0%)
```

Unsupported *syntax*, by contrast, is refused outright — `otag:ramp` names
itself as an error rather than quietly matching nothing, and where the accepted
values are a closed set the message lists them:

```
-engine test simple-ramp.txt typo.criteria.js
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
once said it was not. The index is keyed by lowercased card name, Scryfall
prints tokens that share a name with a real card, and the token could win the
key — at which point every field belonged to the token: mana value 0, no colour
identity, `not_legal` in every format. Forty-one real cards were shadowed that
way, Mutavault and Meteorite and Storm Crow among them.

That is fixed at the source now that `sync` builds the index here: a token is
not a card and never enters it. The defence downstream stays, because an old
index is still readable and the reasoning has not changed — an entry whose type
line says `Token` is treated as no data by every rule that consults card data,
and draws no complaint of any kind. Only deck size still applies to it, because
counting lines in a decklist needs no card data at all. Reporting the token's
fields as the card's would be the same confidently wrong claim as reading a
missing field as guilt.

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
writes the JSON contract above, reads Scryfall's bulk data and reads and writes
the cards in the index, and `figue` parses argv. One `#[derive(Facet)]` per type feeds
all of them.

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
