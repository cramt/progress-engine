# Chancellor of the Spires

A rules engine for Magic. It answers **what is each object on the
battlefield right now**, through the layer system (CR 613), with counters,
and it plays the first slices of a game, Commander or two-player
constructed: mulligans, turns, priority, land drops, mana abilities, and
casting and resolving spells, the commander among them, the first
instants' and sorceries' effects and the first triggered abilities, as a
state machine that stops at every decision ([ADR-0043](../../docs/adr/0043-chancellor-plays-a-game-as-a-state-machine-that-stops-at-every-decision.md)).

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
library, 40 life, seven cards, everyone keeping. `Game::start(Setup)` also
sets up a constructed game (20 life, CR 103.4, and no commander) and asks
for mulligans: `Mulligan { player }`, answered `Mulligan(bool)`, in turn
order, then `Bottom { player, count, hand }` for each player who took one,
as the London mulligan has it (CR 103.5, 103.5c).

A card can be **hidden**: in a library or a hand, with no one having said
what it is, as a replayed log's draws are (below). Its text, cost and
what the engine runs of it exist only on a known card, so a hidden card
can't be played, cast or discarded or have a characteristic read, and
`Game::card` is `None` for it. `Game::reveal` names it once, which is
sound because nothing in the game could have told the difference before.
A deck's `hidden` count is how many it brings; a deck file's cards are all
known, so a goldfish never has one. `deck::read` makes a
player's cards from a `.deck.toml` and the card index (`decks/index.jsonl`),
`deck::card` one card by name. The engine then runs until a player has to
choose and returns a `Decision`, and `game.step(Answer)` takes an answer
among the legal ones, refuses any other with the game unchanged, and runs
on. Turn-based and state-based actions are never decisions.

- `Priority { player, actions }`: pass, play a land, `Cast(card)` from
  hand or a commander from the command zone, activate a mana ability
  (`Mana(ManaActivation { source, ability, adds })`, one action for each
  mana its text lets it add), or activate another activated ability
  (`Activate(Activation { source, ability })`), whose costs the engine
  pays as it goes on the stack.
- `PayOrEnterTapped { player, card, life }`: a shock land's "As this land
  enters, you may pay 2 life. If you don't, it enters tapped", asked
  before it enters (CR 614.1c, 614.12a) and only of a player with the life
  to pay (CR 119.4), answered `PayLife(bool)`.
- `Targets { player, card, slots }`: each target slot's legal targets,
  asked as a spell is cast, before its cost (CR 601.2c), and as a
  triggered ability is put on the stack (CR 603.3d), answered
  `Targets(Vec<Target>)`, a player or a permanent per slot.
- `Order { player, triggers }`: a player's triggered abilities that
  triggered together, put on the stack in the order they choose (CR
  603.3b), answered `Order` with a permutation.
- `ChooseFromHand { player, whose, hand, legal, unsure }`: Thoughtseize's
  "You choose a nonland card from it", answered `Pick`; `None` only when
  no card can match, none hidden and none the engine can't read
  (CR 701.20a).
- `Scry { player, cards }`: answered `Scry { top, bottom }` (CR 701.22a).
- `Search { player, filter, known, hidden }`: a resolving ability searches
  the player's library (CR 701.23a), answered `Find(Found)`: `Nothing`
  (CR 701.23b), a known card, or `Hidden(card, text)`, a hidden card of
  the library named as it is found. Either must match the filter, a
  card's own types and supertypes ("a Mountain or Forest card", "a basic
  land card"), or the answer is illegal.
- `Pay { player, spell, owed, actions }`: the spell is on the stack and
  its total cost determined (CR 601.2a–f). The answers are `Payment::Mana`
  (a mana ability, CR 601.2g), `Payment::Spend(mana)`, offered for every
  amount of the pool that pays `owed` exactly (CR 601.2h; which mana paid
  which symbol changes nothing modelled), and `Payment::Cancel`, which
  puts the game back as it was before the cast, mana abilities included
  (CR 733.1). A cost no pool pays offers only `Cancel`.
- `Discard { player, count, hand }` in cleanup (CR 514.1) or as an
  instruction says (CR 701.9a),
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
mana, a choice of it, any colour, a colour of the commander's identity
(CR 903.4), or mana for each permanent a filter matches, counted as it is
activated (Gaea's Cradle, CR 608.2h), without the stack (CR 605.3b), a
creature's only once it has been yours since your turn began (CR 302.6)
or has haste; other activated abilities any time you have priority (CR
117.1b), costing a tap, life and sacrificing their source (CR 602.2,
701.21a), on the stack and resolving independently of their source (CR
113.7a), so far with the fetchlands' effects: searching your library for
a card a filter matches, putting it onto the battlefield, tapped if the
text says, and shuffling (CR 701.23, 701.24a); instants' and sorceries'
instructions in order (CR 608.2c), going to the graveyard after (CR
608.2n): draw, discard, a hand revealed and a card chosen from it to
discard, gain and lose life, damage to any target, a player, a player or
planeswalker or a creature (CR 120.3, lethal damage and deathtouch as
state-based actions, CR 704.5g–h, removed in cleanup, CR 514.2, with
lifelink, CR 702.15b), scry and +1/+1 or named counters; their targets
chosen as they are cast, and checked again as they resolve, an illegal
one left alone and a spell with every target illegal not resolving (CR
608.2b); triggered abilities that trigger when a permanent a filter
matches enters, itself or "another Human you control" (CR 603.6a), put on
the stack the next time a player would receive priority, the active
player's first, each player's in the order they choose (CR 603.3b),
with their targets, removed if a target slot has no legal choice (CR
603.3d), and resolving independently of their source (CR 113.7a); a basic land
type's intrinsic mana ability (CR 305.6), which the layers grant after
layer 4, so Yavimaya's Forests tap for {G}, Blood Moon's Mountains for {R}
alone (CR 305.7), and a land that loses all abilities for nothing; "enters
tapped", "unless you control" or "if you control" a number of permanents
a filter, or any of its alternatives, matches ("two or fewer other lands",
"two or more basic lands", "a Mountain or a Plains"), not counting the
land itself, and a shock land's life or tapped, all read off the
permanent as it would enter (CR 614.12); mana pools emptying as each step ends (CR 500.5);
state-based actions whenever a player would get priority (CR 704.3) for 0
life, drawing from an empty library, toughness 0 or less and the legend
rule (CR 704.5a–b, 704.5f, 704.5j), and a commander's return to the
command zone (CR 903.9a); a loser leaving the game with what they own and
a multiplayer game going on without them (CR 800.4a).

A card the game can't run is refused by name when played or cast rather
than run without what it can't do: an instruction with no lowering, named
by its node (`Effect::Destroy`, `Effect::May(Move)`), a triggered ability
on any other event (`Trigger::When(Event::Attacks)`,
`Trigger::At(Step::Upkeep)`), a reference the ability's binders don't
resolve to a target or its source, a static that isn't
"enters tapped", a mana ability with a cost or effect beyond those above,
`{X}`, Phyrexian or snow mana, a planeswalker or battle (their counters),
an Aura (its target), and a keyword other than flash, cycling, transmute,
partner and combat's (reach, menace, first and double strike, defender),
which do nothing until something attacks; cycling, transmute and
flashback are carried, never offered. A non-mana activated ability
the engine can't run (a target, a mana cost, a limit, another effect) is
carried and never offered, so the card plays and the ability doesn't.

**35 of Loam's 90 distinct cards are played or cast and resolved, 15 of
them with every ability running** (the basics, Birds of Paradise, Elvish
Mystic, Command Tower, Karplusan Forest, Yavimaya, the fetchlands Misty
Rainforest, Prismatic Vista, Scalding Tarn and Wooded Foothills, and the
shock lands Breeding Pool, Steam Vents and Stomping Ground), measured
2026-11-02, Deep Analysis the one more since it draws two, its flashback
carried:
`cargo test -p chancellor-of-the-spires --test game loam_coverage --
--nocapture` plays each in a game of its own and prints the rest by the
node refused. The most refused: `Effect::Move` and
`Trigger::When(Event::Discards)` (4 each), `Effect::Search` (3), then
destroy, mill, modal spells, additional costs and loyalty (2 each). `tests/game.rs` scripts the rules above, and plays
Loam against a goldfish for six turns, casting what it can.

Not yet: most instructions (destroy, exile, return, create a token,
counter a spell, modal spells, "may", "if"), X, targeting a spell,
activated abilities with targets, mana costs, limits or effects other
than a search for a card, triggers on any event but entering, "intervening
if" (CR 603.4), a card that triggers from a graveyard, declaring
attackers (the declare blockers and combat damage steps aren't in `Step`
until something can attack, CR 508.8), and a land's back face (CR 712). State-based actions that give a choice run one after the other
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
explicit list of statics (costs, enters tapped, a shock land's choice, restrictions, replacement
and prevention effects, `lower::not_characteristics`). A reference in a
static ("its mana value") is read through the `Let` that binds it, and an
ability carried unread keeps its `Let`, its targets and trigger objects
bound for the state machine. Any other node is
refused by name, never dropped. `insight card <name>` shows what a card
reads as.

**522 of the 574 cards in the Modern metagame lower whole** (548 of 600
faces), measured 2026-11-02 against the Scryfall bulk file:
`cargo test --release -p chancellor-of-the-spires --test coverage -- --ignored --nocapture`
prints the number and the refusals by node. It was 495 until a shock
land's "As this land enters, you may pay 2 life" (19 cards) and a static
"if" over rules alone ("If you control two or more other lands, this land
enters tapped", 8) were carried, since neither changes a characteristic.
The most refused are "as long as" (16), a condition not modelled yet;
then modifying by an amount, "as this enters" (6, other choices, or a copiable effect if
it sets a size, CR 613.2a), 7a's characteristic-defining abilities
(`SetPt`) and copy effects. Karn Liberated and two others don't parse:
they are cards with a rule of their
own, which Insight's headline leaves out.

## Real games

The engine's acceptance test will be real games replayed
([ADR-0043](../../docs/adr/0043-chancellor-plays-a-game-as-a-state-machine-that-stops-at-every-decision.md)):
Magic Online's own logs of matches PDBot watched, from
logs.pennydreadfulmagic.com, each line a play the client already judged
legal.

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

### Replay

`chancellor-logs replay [--top N] [--game ID]` plays the cached games
through the engine with `replay::replay`: a two-player constructed game,
20 life, the starting player from "chooses to play first" (or a skipped
first draw, or turn 1's player), the London mulligans the log shows, and
each library **60 hidden cards** (the log never says the deck size; 60 is
constructed's minimum, CR 100.2a). A card is named as it first shows up in
a hand: played, cast or discarded. Then each line becomes answers: priority
passed until the turn and step the line can happen in (the engine's turn
has to be the log's, counting each player's turns), a land played, a spell
cast with its payment found by searching the mana abilities the engine
offers (fewest first, the least flexible sources tapped first, and an
earlier payment chosen again if a later spell can't be paid), the draw
step's draw on the logs that print it, the cleanup discard, and an ability
activated. A spell or ability on the stack resolves before the next line
unless that line targets it. Chat, spectators and the client's notices are
the only lines passed over.

A fetchland's search is the log's to say and it doesn't: it prints the
activation, and a failed search ("fails to find"), never the card found,
which a basic land rarely shows again. So unless the log says the search
failed, the replay answers it with a hidden card of the library named as
it is found, choosing as it chooses a payment: each basic land, the colours
the player's spells this game need most first, then each land the log
shows the player playing anywhere in the match (their deck has it), the
engine refusing any that doesn't match what was searched for; and if a
later spell can't be paid for, it goes back and finds the next. A shock
land's life is the same: no line says it was paid and life totals aren't
printed, so the replay doesn't pay and the land enters tapped, unless that
leaves a later spell unpaid for, when it goes back and pays. Life is paid
only when something that follows needs the land untapped. The game it
replays is one consistent with the log, not necessarily the one played.

A spell's targets are the log's: a player by their seat, a card by a
permanent of that name among the legal targets, the first when two share
the name, since nothing the log says tells them apart. A hand revealed to
Duress or Thoughtseize names its hidden cards: the log prints the hand,
after the discard in older logs and all of it in newer ones, and the card
discarded, and the engine's hand must have as many cards or the replay
stops (`HandSize`). The cards a scry looks at are hidden alike, so the
first as many as the log says go back on top and the rest to the bottom.
A triggered ability goes on the stack because the rules put it there,
never because a line says so, and each "puts triggered ability from [X]
onto the stack" line is matched to one of the engine's: a trigger the
engine puts on the stack that no line shows, or one a line shows that the
engine never triggered, stops the replay (`UnloggedTrigger`,
`Untriggered`), and a trigger's targets and the order of several are read
from the lines that show them. A draw "with" a spell or ability is the
engine's own as it resolves.

The first line the engine won't follow ends the game's replay with a
typed `Stop`: a card refused by the node of its text the engine can't run,
an answer it calls illegal, a state that disagrees with the log (a play it
never offers that turn, a spell it can't pay for, a turn out of step), or
a kind of line not followed yet (an attack, an exile). `--game ID` prints one game line by line with what the engine
did; the report ranks stops by games, with an example line, players
renamed P1 and P2.

**Of 1,061 games replayed, 18 replay to the end** (15 to the log's winner
line), 5 of them past the first turn, measured 2026-11-02; it was 15
before spells' effects and enters triggers ran. They are the
1,299 games that parse whole and that Insight reads whole, less 238 with
no turn in them (empty logs, concessions during the mulligans). 627 stop
in the first turn, 300 in the second, 35 in the third, 7 later, and 74 at
the setup (21 logs name one player, 6 begin mid-game, 3 aren't London
mulligans, 2 disagree on who started). Lands stop few games now:
fetchlands, shock, fast, battle and check lands all run, and the most a
land stops is Cloudpost (35) and Fabled Passage's "then if you control
four or more lands, untap that land" (8). A refused spell or trigger is
ranked by the node it needs: before they ran, `Effect::RevealHand` (86
games: Inquisition of Kozilek, Duress, Thoughtseize),
`Trigger::When(Event::Enters)` (94: Champion of the Parish, Temple of
Deceit, Wall of Blossoms), `Trigger::When(Event::Attacks)` (54),
`Effect::Draw` (35), `Effect::Scry` (23), then
`Trigger::When(Event::DealsDamageTo)` (26), `Trigger::At(Step::Upkeep)`
and `Trigger::When(Event::IsDealtDamage)` (24 each). The stops now, by
games: `Trigger::When(Event::Attacks)` (57, Goblin Guide and Scorch
Spitter first), exalted (40), "as long as" and Cloudpost's count "on the
battlefield" (37 each), a Leyline from the opening hand (35), an upkeep
trigger (33), an attack (33), a discard the engine doesn't ask for and an
exile line not followed (32 each), a card with two faces (30), and the
damage triggers of Goblin Lackey and Jackal Pup (28, 25), `Effect::LookAtTop`
(27) and prowess (25). Combat and what it triggers is the biggest of them
together. None is the engine disagreeing with Magic Online; those that
were are fixed: Gaea's Cradle's "Add {G} for each creature" not read as a
mana ability (CR 605.1a), and Champion of the Parish's "another Human"
missing a Human, since a creature type said with no card type was read
as no creature's (CR 205.3m). 10 games stop on a spell no land pays for
after a fetchland found what the replay chose, where the land it found
was one the log never shows played (a Watery Grave from Polluted Delta).
`tests/replay.rs` replays hand-written logs, a short game to its end and
the plays that must stop it: a second land, a spell no land pays for, a
land in the opponent's turn; a fetchland whose search the replay answers
again when the first land it found can't pay for a later spell, and one
the log says found nothing; a shock land paid for only when a spell
that turn needs it; and Duress's revealed hand and Champion of the
Parish's trigger, which must each match the engine's. `tests/effects.rs`
runs Thoughtseize, Lightning Bolt (at a player, at a creature, and at one
already gone), Preordain, Champion of the Parish and Temple of Deceit.

## Speed

On the research's game-scale board (300 objects; two layer 4 effects, 14
setters and 14 anthems), one from-scratch run takes **1.1 ms** natively
(release, `timing_on_the_research_board`, ignored by default). The research's
toy took 0.34–0.51 ms but never re-decided its order after each effect.
Layer 7 skips the dependency check, since nothing a filter reads changes
there; without that the same board took 53 ms. Not measured in wasm yet.
