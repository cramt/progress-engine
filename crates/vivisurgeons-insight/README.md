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
cargo run --release -p vivisurgeons-insight -- roundtrip               # or: roundtrip 2000, roundtrip --golden
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
| 8 | The Modern metagame: suspend, energy, reflexive triggers that keep their target, one-shot shields, delayed triggers for a turn, domain and converge, tiered modes | 67.5% | 79.5% | **100%** | 89.0% | 95.6% |

There are 33,427 cards and 60,603 abilities. After round 8 the grammar is
about 6,700 lines in `src/parse/` over a 2,400-line AST, and the corpus runs
in about 16 s.

Rounds 1–4 were written by hand against the whole corpus. Rounds 5–8 were
written by agents working through `insight failing N` (the failing abilities
among the EDHREC top N, in rank order) or `insight failing modern-meta`.
Each round was checked against a snapshot of every tree from before it
(`insight snapshot`). No tree was lost, and every tree that changed was read
and judged a correction.

## Milestones

Each milestone is a set of cards that must all parse. `insight coverage`
reports every one. The lists in `milestones/` say where they came from, and
`insight failing <name>` is the worklist for one.

| Milestone | Cards | Abilities | Status |
|---|---|---|---|
| EDHREC top 1000 | 990/990 | 1800/1800 | done |
| Standard after the early-2027 rotation | 2704/3517 (76.9%) | 6175/7054 (87.5%) | |
| Modern metagame: every card in MTGGoldfish's 60 archetypes, sideboards included | 571/571 | 1164/1164 | done |
| All of Standard | 3717/5157 (72.1%) | 8811/10420 (84.6%) | |
| EDHREC top 2000 | 1713/1986 (86.3%) | 3532/3847 (91.8%) | |
| EDHREC top 5000 | 3690/4975 (74.2%) | 8605/10100 (85.2%) | |

Cards tagged `unique-cr-reference` are left out of the lists too, so Karn
Liberated and Trinisphere show up as not matched.

A finished milestone is pinned: `insight pin 1000` writes every face's text
and trees to `golden/edhrec-1000.txt` (and `insight pin modern-meta` to
`golden/modern-meta.txt`), and `tests/golden.rs` reads each one
again and fails if a tree moved. Read each moved tree in the failure, then
bless the change with `UPDATE_GOLDEN=1 cargo test -p vivisurgeons-insight
--test golden` and commit the file with it, so the diff shows what changed.
The test needs no bulk file, so CI holds the grammar to it.

## The round trip

A tree is meant to hold everything the text says that matters to the game:
every subject, zone, quantity, duration, condition and back-reference. A tree
that lost such a word cannot say it again, so `src/unparse.rs` renders every
tree back into Oracle text and compares it with the line it came from. It
renders from the typed nodes alone; nothing in a tree holds source text.

The comparison reads both sides with the lexer, so reminder text, case,
whitespace and the card's name as `~` are already the same, and "a" equals
"an". Everything else it forgives is built in one module, `syn`, and each
entry says why its forms mean the same. Most are English: agreement and
punctuation (a number in digits or words, "it" or "them", how the clauses of
one ability are joined, CR 113.2c and 608.2c), ellipsis and word order (a
subject or verb said once for two clauses, "if" before or after its clause),
back-references ("it", "that card", "the token"), and words the rules read
the same with or without ("each of", "also", "whichever comes first"). The
rest the Comprehensive Rules make identical: "when"/"whenever" (CR 603.1),
"enters"/"enters the battlefield" (CR glossary), "first"/"precombat main
phase" (CR 505.1a), whose library a card goes to and "its graveyard" as its
owner's (CR 400.3), "put"/"return" (CR 400.6), "remains on the
battlefield" (CR 400.7), "protection from each color" (CR 105.1),
"protection from A and from B" (CR 702.16g), "has base power and
toughness"/"becomes" (CR 613.4b), "It's still a land" as its own sentence
(CR 205.1b), and "When X and whenever Y" (CR 603.1b).

The line between the tree and that list is the game: a difference that
changes what happens (timing, who acts, which zone, past or present, some
or all, may or must) is a node; a difference only in wording is a synonym.
`SEMANTICS.md` records each call.

| Set | Round-trip |
|---|---|
| EDHREC top 1000 (pinned) | 1800/1800 (100%) |
| Modern metagame (pinned) | 1164/1164 (100%) |
| EDHREC top 2000 | 3530/3532 (99.9%) |
| All parsed abilities | 48681/48728 (99.9%) |

`tests/roundtrip.rs` holds every pinned ability to it. A second test renames
each pinned card, in its name and in its text (the whole name, and a leading
part of a legendary's name, CR 201.5c), and fails if any tree moves.

Making the top 1000 round-trip found 18 kinds of tree that parsed completely
and meant something else, and 60 of the 1800 pinned trees moved when they
were fixed:

- **Misread.** "Otherwise" read as "If you don't" (it is the other branch of
  an "if", not an option declined). "It's an enchantment" read as "becomes"
  (a returned permanent is that as it enters, CR 611.2e). "If it was a
  creature" and "if it had counters on it" read in the present. "That many
  plus one counters are put on it" read as "you put". "Any of that mana is
  spent on" read as all of it. "Cards revealed this way" read as "from among
  them". Cloud Key's "choose artifact, creature, …" read as choosing objects.
- **Dropped.** "The next end step" and "the next turn's upkeep" read as "the
  end step". "Any number of target creatures" read as "up to X". "Doesn't
  untap during your untap step" and "… its controller's" read alike.
  "Equip legendary creature {3}" dropped "legendary creature". "Combat damage
  to a player or planeswalker" dropped the planeswalker. "The next spell you
  cast this turn" read as one already cast. "That damage" read as "the
  damage dealt this way". "A second target" read as "another target", which
  CR 115.3 does not make the same.
- **Referents.** "That color" and "the chosen color" are told apart, since
  inside "for each color" the first is each pass's color; "the other" is what
  the earlier choice left, not "it"; "this creature or another" keeps
  "this creature".

Beyond the top 1000 it found more: "Each player shuffles …, then draws
seven cards" read as *you* draw, "choose a nonland card name" dropped
"nonland", and "second main phase" read as "postcombat" (CR 505.1b counts
main phases). Making the Modern metagame parse found more trees the grammar
already read wrongly: "cards you don't own" read as "cards your opponents
own" (in Two-Headed Giant a teammate owns cards too), "choose target
opponent" as "choose an opponent" (which does not target), a Saga's "II —"
as an ability word, a sorcery's "The next Dragon spell you cast costs …" as
a static, and a bare "its graveyard" as its controller's (CR 400.3 makes it
the owner's). The 47 corpus abilities that still do not round-trip are
listed by `insight roundtrip`.

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
needs every one of its abilities, so one unusual line sinks it. After round 4
two thirds of abilities parsed but fewer than half of cards; after round 8,
four in five abilities and two in three cards.

**Aiming at a list breaks the plateau for that list.** Rounds 5–7 took the
EDHREC top 1000 from 60% to 100%, and the corpus still rose 14 points, because
a template written for a played card is shared with unplayed ones. After round
7, 15,186 abilities fail, in 13,842 distinct shapes: still 91% one of a kind.
The corpus tail did not get more regular; it got smaller. Round 8 did the
same for the Modern metagame, 67% to 100% of its cards, and the corpus rose
another 6.8 points. After it, 12,419 abilities fail, in 11,410 distinct
shapes.

**Played cards are more regular.** The EDHREC top 1000 sits 14 points above
the corpus, and Loam's list sits 29 points above. Lantern sits at 58%, because
it is built from odd cards: *Players play with the top card of their
libraries revealed*, Sensei's Divining Top, Mana Drain, Chaos Warp.

After round 4, the tokens failing lines broke at were general grammar: `,`
(1,445), `.` (842), `turn` (520), `or` (459). After round 7 they are still
`,` (1,341) and `.` (1,038), and after round 8 `,` (1,281) and `.` (819).
`insight coverage` prints the full list.

## Verdict

**A grammar gets about two thirds of Magic's abilities and plateaus.** It does
not get to all of them. The remaining third is mostly one-off sentences, so
the work stops being "write the grammar" and becomes "write each card".

That rules out *parse Oracle text and you have an engine* for all of Magic.
For a **fixed card pool** it is doable, and round 7 showed it: the 1000 most
played Commander cards all read into typed trees, with no catch-all and no
per-card override. The last 112 included Opposition Agent, Torment of
Hailfire and Darksteel Mutation, and each took a general rule, not a special
case. Round 8 did the same for the 571 cards of the Modern metagame, from
Suspend to Teferi's sorcery timing. The milestones above are the next pools.
Lantern still has 11 failing cards and Loam 4, and the same approach would
finish both.

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
  parse that is complete and wrong; the round trip catches the ones that
  lost or misread a word, but not a tree whose words all come back with the
  wrong node under them.
