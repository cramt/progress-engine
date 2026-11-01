# Chancellor of the Spires

A rules engine for Magic. It answers **what is each object on the
battlefield right now**, through the layer system (CR 613), with counters,
and it plays the first slices of a game of Commander: turns, priority,
land drops, mana abilities, and casting and resolving permanent spells,
the commander among them, as a state machine that stops at every
decision ([ADR-0043](../../docs/adr/0043-chancellor-plays-a-game-as-a-state-machine-that-stops-at-every-decision.md)).
No spell's effect runs yet.

[docs/research/rules-engine.md](../../docs/research/rules-engine.md) is the
plan, and [ADR-0037](../../docs/adr/0037-chancellor-lowers-insights-trees-into-its-own-effects-and-applies-them-layer-by-layer.md)
the seam:

- The game is a typed Rust state machine. `model::State` is the battlefield
  it will hold: objects with their printed characteristics and timestamps,
  counters, attachments, static abilities, and the continuous effects
  resolved spells left with the set each affects fixed (CR 611.2c).
- A continuous effect is the engine's own `Op`, one per layer, and the
  variant is the layer. `lower::card` reads a card's Oracle text through
  Insight and lowers the tree into these, refusing by name any node it
  has no lowering for. Triggered and activated abilities are carried
  unread, so an effect can still remove them.
- `characteristics(&State)` applies them. Rust walks the layers in CR
  613.1's order and applies one effect at a time (CR 613.8c); before each,
  an ascent program picks which: dependency (CR 613.8a), loops (CR 613.8b)
  and timestamps (CR 613.7). An effect that started keeps its set through
  later layers, even when its ability is removed (CR 613.6).
- It builds for `wasm32-unknown-unknown`, so Meldweb Curator can carry it;
  natively ascent's `par` is on.

## The game

`game::Game::new(decks, seed)` sets up Commander (CR 903.6–903.7): each
commander in the command zone, the rest shuffled by the seed into the
library, 40 life, seven cards, everyone keeping. `deck::read` makes a
player's cards from a `.deck.toml` and the card index (`decks/index.jsonl`),
`deck::card` one card by name. The engine then runs until a player has to
choose and returns a `Decision`, and `game.step(Answer)` takes an answer
among the legal ones, refuses any other with the game unchanged, and runs
on. Turn-based and state-based actions are never decisions.

- `Priority { player, actions }`: pass, play a land, `Cast(card)` from
  hand or a commander from the command zone, or activate a mana ability
  (`Mana(ManaActivation { source, ability, adds })`, one action for each
  mana its text lets it add).
- `Pay { player, spell, owed, actions }`: the spell is on the stack and
  its total cost determined (CR 601.2a–f). The answers are `Payment::Mana`
  (a mana ability, CR 601.2g), `Payment::Spend(mana)`, offered for every
  amount of the pool that pays `owed` exactly (CR 601.2h; which mana paid
  which symbol changes nothing modelled), and `Payment::Cancel`, which
  puts the game back as it was before the cast, mana abilities included
  (CR 733.1). A cost no pool pays offers only `Cancel`.
- `Discard { player, count, hand }` in cleanup (CR 514.1),
  `ReturnCommander { player, card }` (CR 903.9a, answered
  `CommandZone(bool)`), `LegendRule { player, among }` (CR 704.5j,
  answered `Keep(id)`), and `Over`.

Covered: every step of the turn with priority, all players passing with
the stack empty ending the step (CR 117.4, 500.2) and with it non-empty
resolving the top (CR 405.5, 608.1), after which the active player gets
priority (CR 117.3b); untap (CR 502.3); the starting player skipping their
first draw in a two-player game and no one in a larger one (CR 103.8a,
103.8c); one land a turn, only in your main phase with priority and the
stack empty (CR 305.1–305.2); casting (CR 601.2) at sorcery speed, or any
time you have priority for an instant or a card with flash (CR 117.1a);
mana costs of generic, coloured, colourless and hybrid symbols (CR 107.4,
202.1a); commander tax (CR 903.8); a permanent spell entering under its
caster's control (CR 608.3a) and an instant or sorcery going to the
graveyard (CR 608.2n); mana abilities that tap, pay life (CR 119.4) and
deal their controller damage, as painlands do (CR 120.3a), adding fixed
mana, a choice of it, any colour, or a colour of the commander's identity
(CR 903.4), without the stack (CR 605.3b), a creature's only once it has
been yours since your turn began (CR 302.6) or has haste; a basic land
type's intrinsic mana ability (CR 305.6), which the layers grant after
layer 4, so Yavimaya's Forests tap for {G}, Blood Moon's Mountains for {R}
alone (CR 305.7), and a land that loses all abilities for nothing; "enters
tapped", and "unless you control" a filter, read off the permanent as it
enters (CR 614.12); mana pools emptying as each step ends (CR 500.5);
state-based actions whenever a player would get priority (CR 704.3) for 0
life, drawing from an empty library, toughness 0 or less and the legend
rule (CR 704.5a–b, 704.5f, 704.5j), and a commander's return to the
command zone (CR 903.9a); a loser leaving the game with what they own and
a multiplayer game going on without them (CR 800.4a).

A card the game can't run is refused by name when played or cast rather
than run without what it can't do: a spell with effects (lowering them
into actions is the next slice), a triggered ability, a static that isn't
"enters tapped", a mana ability with a cost or effect beyond those above,
`{X}`, Phyrexian or snow mana, a planeswalker or battle (their counters),
an Aura (its target), and a keyword other than flash, cycling, transmute,
partner and combat's (reach, menace, first and double strike, defender),
which do nothing until something attacks. A non-mana activated ability is
carried and never offered, so a fetchland plays and can't fetch.

**31 of Loam's 90 distinct cards are played or cast and resolved, 8 of
them with every ability running** (the basics, Birds of Paradise, Elvish
Mystic, Command Tower, Karplusan Forest, Yavimaya), measured 2026-11-01:
`cargo test -p chancellor-of-the-spires --test game loam_coverage --
--nocapture` plays each in a game of its own and prints the rest by the
node refused. The most refused: a spell's effects (20), triggered
abilities (16, the commander among them), shock lands' "as this enters"
(3), additional costs, loyalty, conditional statics and playing lands from
a graveyard (2 each). `tests/game.rs` scripts the rules above, and plays
Loam against a goldfish for six turns, casting what it can.

Not yet: any spell's effect, activated abilities other than mana
abilities, triggers, declaring attackers (the declare blockers and combat
damage steps aren't in `Step` until something can attack, CR 508.8),
mulligans, a land's back face (CR 712), and shock lands' choice as they
enter. State-based actions that give a choice run one after the other
rather than at once (CR 704.3), which nothing can tell apart without
triggers.

## What the layers cover

Layers 4 (adding types and subtypes, setting a basic land type, CR 305.7),
5 (colour), 6 (granting, removing, removing all) and 7b–7d (setting,
modifying, switching), counters in 7c, filters over type, supertype,
subtype, colour, controller, "nontoken", "other", "this" and "enchanted".
`tests/layers.rs` has CR 613.4d, 613.5, 613.6 and 613.9's worked
examples, built by hand since their cards are hypothetical. `tests/oracle.rs`
reads real cards from their Oracle text: Glorious Anthem with counters,
Giant Growth under Humility, Inspired Charge's fixed set against Glorious
Anthem (CR 611.2c), Blood Moon with Urborg and Humility with Opalescence in
both timestamp orders against their Gatherer rulings, March of the Machines
(CR 613.6, 205.1b), Crusade, and Humility with Toph, the First Metalbender,
including Toph's own earthbend (CR 701.66a) on a Forest and on a Sol Ring
Toph made a land.

Not yet: layers 1–3 (copy, control, text), 7a (characteristic-defining
abilities, and so CDAs first, CR 613.3), dependency through "what it does"
(CR 613.8a), counters' own timestamps (CR 613.7c), abilities granted with a
later timestamp (CR 613.7a), effects on players and rules (CR 613.10–11),
and conditional statics ("as long as", "if").

## Lowering

`lower::card` lowers the statics `Modify`, `BasePt`, `BasePtEqual` (its
mana value), `Loses` (all abilities), `AlsoIs` with listed types, and `Is`
setting a basic land type or making an artifact creature (CR 205.1b);
filters over type, supertype, subtype, colour, "nontoken", "other", "you
control", "enchanted" and "equipped"; and, as they resolve, `Modify` and
earthbend. Everything else that changes no characteristic is carried as
`Ability::Unread` so an effect can still remove it: triggered, activated,
loyalty and chapter abilities, keywords the model doesn't name, and an
explicit list of statics (costs, enters tapped, restrictions, replacement
and prevention effects, `lower::not_characteristics`). A reference in a
static ("its mana value") is read through the `Let` that binds it, and an
ability carried unread keeps its `Let`, its targets and trigger objects
bound for the state machine. Any other node is
refused by name, never dropped. `insight card <name>` shows what a card
reads as.

**495 of the 574 cards in the Modern metagame lower whole** (521 of 600
faces), measured 2026-11-01 against the Scryfall bulk file:
`cargo test --release -p chancellor-of-the-spires --test coverage -- --ignored --nocapture`
prints the number and the refusals by node. The most refused are "as this
enters" (25, a choice, or a copiable effect if it sets a size, CR 613.2a),
"as long as" (16) and "if" (8), conditions not modelled yet; then 7a's
characteristic-defining abilities (`SetPt`) and copy effects. Karn
Liberated and two others don't parse: they are cards with a rule of their
own, which Insight's headline leaves out.

## Real games

The engine's acceptance test will be real games replayed
([ADR-0043](../../docs/adr/0043-chancellor-plays-a-game-as-a-state-machine-that-stops-at-every-decision.md)):
Magic Online's own logs of matches PDBot watched, from
logs.pennydreadfulmagic.com, each line a play the client already judged
legal. Replaying them is the next slice; so far the corpus is fetched and
read.

- `cargo run --release -p chancellor-of-the-spires --bin chancellor-logs -- fetch --max 3000`
  crawls the Freeform listing (all of it, about 1,660 matches), then Penny
  Dreadful's newest first, and saves each match's export as
  `~/.cache/chancellor/pdlogs/<id>.txt` until the cache holds `--max`
  matches. It skips what is cached, so it resumes, and makes at most one
  request a second. The logs carry players' names and are the site's, so
  they are never committed.
- `log::parse` reads an export into typed events: the header, each game's
  entries with their turn (`Turn 3: alice`) and an actor that is always one
  of the match's players, cards, hidden permanents ("face-down creature
  #1"), targets with their mode or divided damage, and how a spell was
  cast. It is pure text in, so it builds for wasm. A line it can't read is
  `Event::Unrecognised`, never dropped.
- `chancellor-logs coverage` parses the cache and reports the numbers
  below, the unrecognised line kinds by count, and the worklist: the cards
  that keep Insight from reading a game whole, ranked by how many games,
  read from the Scryfall bulk file `scryfall sync` keeps.

**Of 6,934 games in 3,000 matches, 6,562 (94.6%) parse into events whole,
and in 1,336 (19.3%) Insight reads every card the log names** (tokens
aside; 23.0% of Freeform's 3,893 games, 14.5% of Penny Dreadful's 3,041),
measured 2026-11-01. 2,203 of the Freeform games say Modern in their
comment or Gatherling event. The 587 unread lines fall into 247 kinds, none
more than 27 lines: a retarget with "no other legal targets", Timesifter,
proliferate, votes, two events printed on one line. The cards blocking the
most games are Fervent Champion (340), River of Tears (319), Kytheon (251),
Kellan, Planar Trailblazer (237) and Thran Portal (221); 19 names aren't in
the bulk file, mostly tokens the log doesn't call tokens (Ragavan, Marit
Lage), dungeons, and digital-only cards.

## Speed

On the research's game-scale board (300 objects; two layer 4 effects, 14
setters and 14 anthems), one from-scratch run takes **1.1 ms** natively
(release, `timing_on_the_research_board`, ignored by default). The research's
toy took 0.34–0.51 ms but never re-decided its order after each effect.
Layer 7 skips the dependency check, since nothing a filter reads changes
there; without that the same board took 53 ms. Not measured in wasm yet.
