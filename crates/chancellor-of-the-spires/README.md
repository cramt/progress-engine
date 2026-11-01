# Chancellor of the Spires

A rules engine for Magic. It answers **what is each object on the
battlefield right now**, through the layer system (CR 613), with counters,
and it plays the first slice of a game of Commander: turns, priority,
land drops and basic lands' mana, as a state machine that stops at every
decision ([ADR-0043](../../docs/adr/0043-chancellor-plays-a-game-as-a-state-machine-that-stops-at-every-decision.md)).
No spell is cast yet.

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
player's cards from a `.deck.toml` and the card index (`decks/index.jsonl`).
The engine then runs until a player has to choose and returns a
`Decision`: `Priority { player, actions }` (pass, play a land, tap a
land for mana), `Discard { player, count, hand }` in cleanup (CR 514.1),
or `Over`. `game.step(Answer)` takes an answer among the legal ones,
refuses any other with the game unchanged, and runs on. Turn-based and
state-based actions are never decisions.

Covered: every step of the turn with priority, all players passing with
the stack empty ending the step (CR 117.4, 500.2); untap (CR 502.3); the
starting player skipping their first draw in a two-player game and no one
in a larger one (CR 103.8a, 103.8c); one land a turn, only in your main
phase with priority and the stack empty (CR 305.1–305.2); a basic land
type's intrinsic mana ability (CR 305.6), read off the layers, so a land
Yavimaya makes a Forest taps for {G}; mana pools emptying as each step
ends (CR 500.5); state-based actions whenever a player would get priority
(CR 704.3) for 0 life and drawing from an empty library (CR 704.5a–b),
a loser leaving the game with what they own and a multiplayer game going
on without them (CR 800.4a).

A land whose text doesn't lower, or that carries an ability the game
can't run yet (a trigger, "enters tapped", a shock land's "as this
enters"), is refused by name when played rather than played without it.
Seven of Loam's 90 cards don't lower, and its multi-faced card can't be
read from the index, which keeps only its faces' text joined.
`tests/game.rs` plays Loam against a goldfish for three turns from a seed,
and scripts the rules above.

Not yet: casting spells, the stack's contents, costs, activated abilities
other than basic land mana, triggers, declaring attackers (the declare
blockers and combat damage steps aren't in `Step` until something can
attack, CR 508.8), mulligans, and a land's back face (CR 712). A land that loses all
abilities still taps for mana: the intrinsic ability is read off its land
types, not carried as an ability the layers can remove.

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

## Speed

On the research's game-scale board (300 objects; two layer 4 effects, 14
setters and 14 anthems), one from-scratch run takes **1.1 ms** natively
(release, `timing_on_the_research_board`, ignored by default). The research's
toy took 0.34–0.51 ms but never re-decided its order after each effect.
Layer 7 skips the dependency check, since nothing a filter reads changes
there; without that the same board took 53 ms. Not measured in wasm yet.
