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
cargo run --release -p vivisurgeons-insight -- failing 2000            # or a milestone: failing modern-meta
cargo run --release -p vivisurgeons-insight -- snapshot /tmp/trees.txt  # diff two of these around a change
```

It reads `~/.cache/scryfall/oracle-cards.jsonl` and `oracle-tags.jsonl`, the
files `scryfall sync` keeps. `--bulk` and `--tags` point elsewhere. The whole
corpus takes about 16 s on one thread.

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
| 5 | Aimed at EDHREC top 250: reflexive and intervening-if triggers, typed mana constraints, replacement events | 52.4% | 69.0% | 77.7% | 72.0% | 87.8% |
| 6 | Top 500: copy exceptions, `for each` clauses, top or bottom of a library, a subject with a list of predicates | 56.8% | 72.2% | 88.7% | 81.0% | 90.0% |
| 7 | Top 1000: damage doublers, digging until a card, repeated processes, extra phases, aura-made creatures | 60.7% | 74.9% | **100%** | 86.0% | 92.2% |

There are 33,427 cards and 60,603 abilities. After round 7 the grammar is
about 5,100 lines in `src/parse/` over a 1,800-line AST, and the corpus runs
in about 16 s.

Rounds 1–4 were written by hand against the whole corpus. Rounds 5–7 were
written by agents working through `insight failing N`, the failing abilities
among the EDHREC top N, in rank order. Each round was checked against a
snapshot of every tree from before it (`insight snapshot`). No tree was lost,
and every tree that changed was read and judged a correction.

## Milestones

Each milestone is a set of cards that must all parse. `insight coverage`
reports every one. The lists in `milestones/` say where they came from, and
`insight failing <name>` is the worklist for one.

| Milestone | Cards | Abilities | Status |
|---|---|---|---|
| EDHREC top 1000 | 990/990 | 1800/1800 | done |
| Standard after the early-2027 rotation | 2108/3517 (59.9%) | 5419/7054 (76.8%) | |
| Modern metagame: every card in MTGGoldfish's 60 archetypes, sideboards included | 384/571 (67.3%) | 939/1164 (80.7%) | |
| All of Standard | 2889/5157 (56.0%) | 7770/10420 (74.6%) | |
| EDHREC top 2000 | 1633/1986 (82.2%) | 3435/3847 (89.3%) | |
| EDHREC top 5000 | 3340/4975 (67.1%) | 8171/10100 (80.9%) | |

Cards tagged `unique-cr-reference` are left out of the lists too, so Karn
Liberated and Trinisphere show up as not matched.

A finished milestone is pinned: `insight pin 1000` writes every face's text
and trees to `golden/edhrec-1000.txt`, and `tests/golden.rs` reads each one
again and fails if a tree moved. Read each moved tree in the failure, then
bless the change with `UPDATE_GOLDEN=1 cargo test -p vivisurgeons-insight
--test golden` and commit the file with it, so the diff shows what changed.
The test needs no bulk file, so CI holds the grammar to it.

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

**Aiming at a list breaks the plateau for that list.** Rounds 5–7 took the
EDHREC top 1000 from 60% to 100%, and the corpus still rose 14 points, because
a template written for a played card is shared with unplayed ones. After round
7, 15,186 abilities fail, in 13,842 distinct shapes: still 91% one of a kind.
The corpus tail did not get more regular; it got smaller.

**Played cards are more regular.** The EDHREC top 1000 sits 14 points above
the corpus, and Loam's list sits 29 points above. Lantern sits at 58%, because
it is built from odd cards: *Players play with the top card of their
libraries revealed*, Sensei's Divining Top, Mana Drain, Chaos Warp.

After round 4, the tokens failing lines broke at were general grammar: `,`
(1,445), `.` (842), `turn` (520), `or` (459). After round 7 they are still
`,` (1,341) and `.` (1,038). `insight coverage` prints the full list.

## Verdict

**A grammar gets about two thirds of Magic's abilities and plateaus.** It does
not get to all of them. The remaining third is mostly one-off sentences, so
the work stops being "write the grammar" and becomes "write each card".

That rules out *parse Oracle text and you have an engine* for all of Magic.
For a **fixed card pool** it is doable, and round 7 showed it: the 1000 most
played Commander cards all read into typed trees, with no catch-all and no
per-card override. The last 112 included Opposition Agent, Torment of
Hailfire and Darksteel Mutation, and each took a general rule, not a special
case. The milestones above are the next pools. Lantern still has 14 failing
cards and Loam 7, and the same approach would finish both.

Two warnings for anyone who builds on this:

- **A parse is not semantics.** Until round 5, `IfYouDo` also held "When you
  do", which is a reflexive trigger in the rules (CR 603.12), not a condition.
  The tree says what the text says, and choosing its nodes is already a
  claim about the rules. An engine still has to give each node a meaning
  under the layers, the stack and replacement rules.
- **Greedy rules mislead silently.** Twice a list rule ate a word that
  belonged to the next clause (`any target and you gain 3 life`, `lands from
  your graveyard`). A grammar this size needs a sample read by eye after every
  round, as `insight sample` does. A coverage number alone does not catch a
  parse that is complete and wrong.
