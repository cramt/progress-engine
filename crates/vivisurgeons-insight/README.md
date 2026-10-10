# Vivisurgeon's Insight

A spike. It asks one question: **how much of Magic can a grammar read from
Oracle text into a typed ability tree with no hole in it?** It is not a rules
engine. Nothing here runs an ability. It only reads one.

The card is *Draw three cards. Proliferate.* A parser cuts card text open to
see what is inside, and the namesake is the first test: it reads as
`[Draw { who: You, n: 3 }, Action { name: "proliferate" }]`.

## Run it

```
cargo run --release -p vivisurgeons-insight -- coverage --deck decks/lantern.deck.toml --deck decks/loam.deck.toml
cargo run --release -p vivisurgeons-insight -- card Swords to Plowshares
cargo run --release -p vivisurgeons-insight -- sample 25
```

It reads `~/.cache/scryfall/oracle-cards.jsonl` and `oracle-tags.jsonl`, the
files `scryfall sync` keeps. `--bulk` and `--tags` point elsewhere. The whole
corpus takes about 13 s on one thread.

## What counts as parsed

A card counts as parsed only if every ability on every face parses and the
parse consumes every token. No rule swallows "the rest of the sentence". Each
word is read by a rule that gives it a typed meaning, or the line fails.
Reminder text is dropped before parsing. Keyword names, card types, subtypes
and keyword actions come from Scryfall's catalogs in `vocab/`, not from a list
written by hand.

Cards tagged `otag:unique-cr-reference` are left out of every figure. Each
has a rule in the Comprehensive Rules written for it alone, so any engine
handles it as its own special case. The tag holds 70 cards, and 62 of them
are in the corpus after tokens and Un-cards are removed.

## The numbers

| Round | What it added | Cards | Abilities | EDHREC top 1000 | Lantern | Loam |
|---|---|---|---|---|---|---|
| 1 | Lexer, objects, costs, the core effects, triggers, statics | 37.2% | 56.7% | 45.1% | 44.0% | 54.4% |
| 2 | Clause prefixes and suffixes (`may`, `unless`, `for each`, `where X is`, `if you don't`, delayed steps), `as ~ enters`, `becomes`, prevention, replacement effects | 43.7% | 62.3% | 57.8% | 58.0% | 68.9% |
| 3 | Conditional statics, `would … instead`, werewolf conditions, Class levels | 45.5% | 64.0% | 58.8% | 58.0% | 70.0% |
| 4 | Fixes for greedy rules (a zone eaten by the object before it), `X or another Y` | 46.3% | 64.6% | 60.3% | 58.0% | 75.6% |

There are 33,427 cards and 60,603 abilities. After round 4 the grammar is
about 1,900 lines of `parse.rs` over a 770-line AST.

After round 4 the parser read an evenly spaced sample of 25 successful parses.
24 were right. The 25th dropped the subject of "it explores". That led to a
fix across the AST: every subject, zone and source the grammar reads is now
kept in the tree. Before the fix, a discarded word still counted as a success.
The numbers did not move, which is what a change that only keeps information
should do.

## What the curve says

**The gain per round falls fast: +6.5, +1.8, then +0.8 points of cards.**
Round 1 had the common templates. Each later round fixed structure, not single
cards, and still bought less.

**The tail is sentences, not templates.** After round 4, 21,463 abilities
fail, in 19,430 distinct shapes, even with digits made equal. 90% of failing
abilities are the only one of their shape. The most common failing shape
covers 38 abilities (*Start your engines!*, which is not in Scryfall's
keyword-ability catalog). By the 40th shape, a shape covers 7. A rule written
for one shape now buys a handful of cards.

**Card coverage is lower than ability coverage, and always will be.** A card
needs every one of its abilities, so one unusual line sinks it. Two thirds of
abilities parse, but fewer than half of cards do.

**Played cards are more regular.** The EDHREC top 1000 sits 14 points above
the corpus, and Loam's list sits 29 points above. Lantern sits at 58%, because
it is built from odd cards: *Players play with the top card of their
libraries revealed*, Sensei's Divining Top, Mana Drain, Chaos Warp.

The tokens failing lines break at are general grammar: `,` (1,445), `.`
(842), `turn` (520), `or` (459). `insight coverage` prints the full list.
That is where the next rounds would go, at the same falling rate.

## Verdict

**A grammar gets about two thirds of Magic's abilities and plateaus.** It does
not get to all of them. The remaining third is mostly one-off sentences, so
the work stops being "write the grammar" and becomes "write each card".

That rules out *parse Oracle text and you have an engine* for all of Magic.
For a **fixed card pool** it is doable. Lantern and Loam together have 75
distinct failing abilities across 62 cards. That is a list someone can write
by hand, as overrides keyed by `oracle_id` beside the grammar's output.

Two warnings for anyone who builds on this:

- **A parse is not semantics.** `IfYouDo` here also holds "When you do", which
  is a reflexive trigger in the rules (CR 603.12), not a condition. The tree
  says what the text says. An engine still has to give each node a meaning
  under the layers, the stack and replacement rules.
- **Greedy rules mislead silently.** Twice a list rule ate a word that
  belonged to the next clause (`any target and you gain 3 life`, `lands from
  your graveyard`). A grammar this size needs a sample read by eye after every
  round, as `insight sample` does. A coverage number alone does not catch a
  parse that is complete and wrong.
