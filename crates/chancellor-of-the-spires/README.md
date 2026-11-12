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
`Game::card` is `None` for it; it can only be cast face down, a 2/2 that
is the same whatever its card (CR 708.2a), and is then named only as a
card with morph. `Game::reveal` names it once, which is
sound because nothing in the game could have told the difference before.
A deck's `hidden` count is how many it brings; a deck file's cards are all
known, so a goldfish never has one. `deck::read` makes a
player's cards from a `.deck.toml` and the card index (`decks/index.jsonl`),
`deck::card` one card by name. The engine then runs until a player has to
choose and returns a `Decision`, and `game.step(Answer)` takes an answer
among the legal ones, refuses any other with the game unchanged, and runs
on. Turn-based and state-based actions are never decisions.

- `OpeningHand { player, cards }`: once the mulligans are done, the
  starting player first, a player whose opening hand has a card that
  lets them "begin the game with it on the battlefield" puts any of
  those there (CR 103.6, 103.6a), answered `Begin`.
- `Priority { player, actions }`: pass, play a land, `Cast(card)` from
  hand or a commander from the command zone, `PlayBack(card)` and
  `CastBack(card)`, a card's other face (below), `CastFaceDown(card)` and
  `TurnFaceUp(permanent)` (below), activate a mana ability
  (`Mana(ManaActivation { source, ability, adds })`, one action for each
  mana its text lets it add), or activate another activated ability
  (`Activate(Activation { source, ability })`), whose costs the engine
  pays as it goes on the stack.
- `PayOrEnterTapped { player, card, life }`: a shock land's "As this land
  enters, you may pay 2 life. If you don't, it enters tapped", asked
  before it enters (CR 614.1c, 614.12a) and only of a player with the life
  to pay (CR 119.4), answered `PayLife(bool)`.
- `DiscardToEnter { player, card, legal, unsure }`: Mox Diamond's "If
  this artifact would enter, you may discard a land card instead",
  answered `Pick`: it enters for the card discarded, and goes to its
  owner's graveyard for `None` (CR 614.1a).
- `EnterAsCopy { player, card, of, may }`: Vesuva's "You may have this
  land enter tapped as a copy of any land on the battlefield", asked
  before it enters and before anything else of how it enters is read,
  since that is read from the copy (CR 707.5, 614.12a, 616.1), answered
  `Copy` with one of `of`, or `None` to enter as itself when `may`.
- `Kicker { player, card, cost }`: a spell with kicker is being cast,
  and its caster announces whether they pay `cost` more (CR 601.2b,
  702.33a), before its targets, answered `Kick(bool)`.
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
- `May { player, card }`: an instruction that says "you may", asked as
  it is reached (CR 608.2d; a triggered ability's as it resolves, CR
  603.5), answered `May(bool)`.
- `Reveal { player, cards }`: hidden cards of a library are revealed
  (CR 701.20a), and what each is must be said, answered `Reveal` with a
  card for each. A deck file's cards are known, so only a replay is asked.
- `ChooseAmong { player, cards, n }`: Impulse's "put one of them into
  your hand", answered `Choose`; `Arrange { player, cards, top }`: put
  cards back on top or bottom in any order (CR 401.4), answered `Arrange`,
  the first topmost.
- `PutFromHand { player, hand, legal, unsure }`: Goblin Lackey's "put a
  Goblin permanent card from your hand onto the battlefield", answered
  `Pick`, `None` only when no card can match.
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
- `Attackers { player, can, defenders }`: the active player declares
  attackers (CR 508.1), answered `Attack` with each attacker and the
  player it attacks, none for no attack; asked only when a creature can
  attack: untapped, under its controller's control since their turn
  began or with haste (CR 302.6), and without defender (CR 702.3b).
- `Blockers { player, can, attackers }`: each defending player declares
  blockers (CR 509.1), answered `Block` with each blocker and the
  attacker it blocks, legal when a creature with flying is blocked only
  by flying or reach and one with menace by two or more (CR 702.9b,
  702.111b).
- `Assign { player, source, power, to, lethal }`: a creature blocked by
  two or more, or with trample, divides its combat damage (CR 510.1c,
  702.19b), answered `Assign` with an amount for each.
- `Discard { player, count, hand }` in cleanup (CR 514.1) or as an
  instruction says (CR 701.9a),
  `ReturnCommander { player, card }` (CR 903.9a, answered
  `CommandZone(bool)`), `LegendRule { player, among }` (CR 704.5j,
  answered `Keep(id)`), and `Over`.

A card can have **two faces** (`deck::Oracle::TwoFaces`): a modal
double-faced card (CR 712.3), a transforming one (CR 712.2) or an
adventurer card (CR 715.1). Each face
is read through Insight as a card of its own, its own name, cost, type
line and text, and which is up is the card's state in the game, a
`deck::Side` that only a card with two faces has (the game's `Face::Two`): its
front in every zone but the stack and the battlefield, where it has only
the front's characteristics (CR 712.8a, 715.4), and the face cast or
played on them (CR 712.8f, 715.3b), turned back to the front as it
leaves. The layers then read the face up like any card. A modal card is
played as a land by its land face (`PlayBack`, CR 712.12) or cast by
either face, only that face evaluated (`CastBack`, CR 712.11b–c). An
adventurer card is cast as its Adventure (`CastBack`, CR 715.3), with
only the Adventure's characteristics on the stack (CR 715.3a–b); as it
resolves it is exiled, and its caster may then cast the creature from
exile, though not as an Adventure again (CR 715.3d); countered, or with
its targets gone, it goes to the graveyard as any spell does. A
transforming card is cast and put onto the battlefield front face up
(CR 712.11, 712.14) and turns over only as an instruction transforms it:
"transform this creature", the same permanent with its other face's
characteristics and a new timestamp, its back face's mana value its
front's (CR 701.27a, 712.8e, 712.18, 613.7g), and what waits for it to
transform into a face triggers (CR 701.27e). An ability names the face
it is printed on, so one on the stack when its permanent transforms is
still its own, and its "transform" does nothing then (CR 701.27f); one
whose back face the engine can't run is refused whole. Delver of
Secrets's "look at the top card … You may reveal that card. If an
instant or sorcery card is revealed this way" reveals the card looked at,
naming it if it is hidden (CR 701.20a), and reads it. A card of another
layout with more than one face (split, flip, prepare …) is refused by
name, "a multi-faced card (split)", and `Side` grows a case where a
layout needs one. The card index keeps a
face's own text only for a card with more than one
(`chip_scryfall::index::Face::oracle`), so an index built before it
plays none of them.

A permanent can be a **copy** (CR 707): its copiable values are then
another object's, the text the game runs of it and the name a line calls
it by, in place of its card's face (`Game::card`). They are layer 1 (CR
613.2a): the layers start every object from its copiable values
(`model::Object::printed`), written as a copy effect begins, the latest
being the one that shows. A permanent entering as a copy is one from the
start (CR 707.5), so the copied text's own "enters tapped", choices and
enters triggers apply (Vesuva as Glimmerpost gains the life), and it is
its own card again in any other zone (CR 400.7). One that becomes a copy
where it is stays the same permanent, nothing entering, its timestamp,
counters and the effects on it kept (CR 707.4): Thespian's Stage's "This
land becomes a copy of target land, except it has this ability", whose
exception adds the ability's own line to the copied rules text (CR
707.9a). Copiable values are kept for the game (`Printed::Copy`), so an
ability of them on the stack is still the copy's once its permanent has
left (CR 113.7a), as one of a face is the face's when it transforms. A
face-down permanent is never copied, a copy effect that ends (Cytoshape's
"until end of turn") isn't lowered, and an exception other than "it has
this ability" is refused by name ("a copy except AlsoIs", Glasspool
Mimic's Shapeshifter Rogue): each is one more case of the same copy.

Covered: every step of the turn with priority, all players passing with
the stack empty ending the step (CR 117.4, 500.2) and with it non-empty
resolving the top (CR 405.5, 608.1), after which the active player gets
priority (CR 117.3b); untap (CR 502.3); the starting player skipping their
first draw in a two-player game and no one in a larger one (CR 103.8a,
103.8c); one land a turn, only in your main phase with priority and the
stack empty, and another for each "You may play an additional land on
each of your turns" its player controls (CR 305.1–305.2a: Exploration); casting (CR 601.2) at sorcery speed, or any
time you have priority for an instant or a card with flash (CR 117.1a);
morph: a card cast face down from hand for {3} at sorcery speed, a 2/2
creature spell and permanent with no name, text, subtypes, colour or
mana cost, which is what cost changes, targets and triggers on spells see
(CR 702.37c, 708.2a, 708.4), and which enters with nothing of its own
applying (CR 708.3); turned face up as a special action any time its
controller has priority by paying its morph cost, without the stack,
the same object with its card's characteristics and a new timestamp, its
controller getting priority again (CR 702.37e, 116.2b, 116.3, 708.8,
613.7f); a card whose text the engine can't run is still cast face down,
and only turning it face up is refused;
mana costs of generic, coloured, colourless and hybrid symbols (CR 107.4,
202.1a); commander tax (CR 903.8); kicker's mana added to the cost before
cost changes (CR 601.2f, 702.33a), and "if this spell was kicked", alone
or as the "instead" that replaces the instruction before it, Burst
Lightning's "it deals 4 damage instead" dealt to the target that
instruction chose (CR 702.33d); a static's generic cost change for
spells a filter matches, cast by anyone, its controller or their
opponents (Thalia's "Noncreature spells cost {1} more", Goblin
Warchief's "Goblin spells you cast cost {1} less"), for each permanent a
filter matches and only during its controller's turn (Temur
Battlecrier's), increases added and
reductions taken from the generic component alone, never below {0} (CR
601.2f, 118.7a); a permanent spell entering under its
caster's control (CR 608.3a) and an instant or sorcery going to the
graveyard (CR 608.2n); an Aura spell targeting what its enchant ability
says, a permanent ("Enchant land", "enchant creature you control", CR
303.4a, 702.5a), entering attached to it as it resolves and not resolving
if it has become illegal (CR 608.3b–c), its "enchanted land" the
permanent it is attached to; mana abilities that tap, pay life (CR 119.4) and
deal their controller damage, as painlands do (CR 120.3a), pay mana
from the pool, as filter lands and Signets do (CR 605.3a), put a counter
on their source or take one off it (Wall of Roots's -0/-1, Gemstone
Mine's mining counter, sacrificed when none are left, CR 602.2b, 118.3), sacrifice their
source or a permanent a filter matches (Lotus Petal, Skirk Prospector's
"Sacrifice a Goblin", CR 701.21a), or are
limited by "Activate only if you control a creature with power 4 or
greater" (CR 602.5) or to once each turn (CR 602.5b), adding fixed
mana, a choice of it, any colour, a colour of the commander's identity
(CR 903.4), or mana for each permanent a filter matches, counted as it is
activated (Gaea's Cradle, CR 608.2h), without the stack (CR 605.3b),
with what the triggered mana abilities a permanent tapped for mana
triggers add, at once and to its controller (Wild Growth's "Whenever
enchanted land is tapped for mana, its controller adds an additional
{G}", CR 106.12a, 605.1b, 605.4a), a
creature's only once it has been yours since your turn began (CR 302.6)
or has haste; other activated abilities any time you have priority (CR
117.1b), or only once each turn or if an opponent lost life this turn
(CR 602.5, 602.5b: Hired Claw), costing a tap, life, sacrificing their source and mana, paid
as a spell's is (CR 602.2, 602.2b, 701.21a), their targets chosen as
they are activated (Mogg Fanatic, Wasteland, Rishadan Port's "Tap target
land", CR 701.26a), "becomes a 2/2 Assembly-Worker artifact creature
until end of turn. It's still a land", which keeps every type it had (CR
205.1b), on the stack and resolving independently of their source (CR
113.7a), with the instructions a spell's are lowered into, among them
the fetchlands' effects, and Aether Vial's, which counts its
charge counters as it resolves, as the Vial last existed if it has left
(CR 608.2h); instants' and sorceries'
instructions in order (CR 608.2c), going to the graveyard after (CR
608.2n), each acting on one object or on each permanent a filter
matches as it is followed ("put a +1/+1 counter on each other Human you
control", "destroy all creatures", "2 damage to each creature", dealt at
once): draw, discard, a hand revealed and a card chosen from it to
discard, the top cards of a library revealed or looked at and bound for
the instructions after them (Insight's `With`, a slot per binder): put
into a hand, those of them a filter matches ("all Goblin cards revealed
this way", Goblin Ringleader) and the rest, one chosen among them, put back on top or the bottom in
any order, "if it's a land card" and "its mana value", destroy (CR
701.8a), exile (CR 701.13a), return to its owner's hand, sacrifice
(CR 701.21a), counter target spell (CR 701.6a; a spell is never its own
target, CR 115.5), and "its controller" and "its power" of a permanent
gone, as it last existed (CR 608.2h), a search of the searching player's own
library for a card a filter matches, bound for what follows (CR 701.23a,
701.23b: it may find nothing), only a card the engine runs if it is put
onto the battlefield, then put onto the battlefield (tapped, under the
searcher's control), into a hand or on top, and a shuffle of any
player's library (CR 701.24a), a "may" being anyone's (Path to Exile's
is its target's controller's), creating a token whose text the
instruction gives, its creator its owner and controller, named for its
subtypes (CR 111.1–111.4), a card refused if a token it makes can't run, gain and lose life, 1 life
"for each Locus on the battlefield" counted as it resolves (Glimmerpost,
CR 608.2h), damage to any target, a player, a player or
planeswalker or a creature (CR 120.3, lethal damage and deathtouch as
state-based actions, CR 704.5g–h, removed in cleanup, CR 514.2, with
lifelink, CR 702.15b), scry and +1/+1, other power and toughness
(CR 122.1a) or named counters, mana added to the pool (Dark
Ritual, CR 106.3), life "for each land card discarded this way", of
the cards the discard bound (Gerrard's Verdict), damage "where X is the number of Goblins on the
battlefield", counted as it is dealt (CR 608.2h), "can't be regenerated", which restricts nothing
while no regeneration lowers (CR 701.19a–c), a card name chosen, of a
real card with the characteristics asked (Cabal Therapy's "a nonland
card name", CR 201.4a), a hand revealed, its hidden cards named (CR
701.20a), "discards all cards with that name" (CR 701.9a), "counter target
spell unless its controller pays {3}" and echo's "sacrifice it unless
you pay" (CR 118.12a, 702.30a: Mana Leak, Goblin Patrol), the payer
activating mana abilities first or declining, an "unless" paid by
discarding a card (Masticore, Solitary Confinement, CR 701.9a) or by
sacrificing any number of creatures with total power 12 or greater
(Phyrexian Dreadnought, itself among them, CR 701.21a), and "then if
you control four or more lands", "that land" being the permanent a card
found became (Fabled Passage); their targets
chosen as they are cast, and checked again as they resolve, an illegal
one left alone and a spell with every target illegal not resolving (CR
608.2b); triggered abilities that trigger when a permanent a filter
matches enters, itself or "another Human you control" (CR 603.6a), put on
the stack the next time a player would receive priority, the active
player's first, each player's in the order they choose (CR 603.3b),
with their targets, removed if a target slot has no legal choice (CR
603.3d), and resolving independently of their source (CR 113.7a), on a
permanent entering, a creature attacking (CR 508.3a), a player, its
controller or an opponent casting a spell a filter matches, "that
player" its caster (CR 601.2i: Eidolon of the Great Revel, Warmth), a
player, you or an opponent drawing a card, "that player" the one who
drew (CR 121.2: Razorkin Needlehead), its owner cycling the card, from
the graveyard it went to (Gempalm Incinerator, CR 702.29a), a permanent being dealt damage, with the
amount for "that much" (CR 120.3; combat damage dealt at once is one
event, CR 510.2), a permanent dealing damage, or combat damage, to a
player, a creature dying, its own "when this creature dies" looking back
at it as it last existed (CR 700.4, 603.6c, 603.10a), a permanent
dealing damage to anything, with all it dealt at once for "that much"
(Exalted Angel, CR 120.3, 510.2), a permanent turned face up (Nantuko
Vigilante, CR 702.37e), its controller
attacking with one or more creatures a filter matches (Hired Claw's
Lizards, CR 508.3a), and the beginning
of an upkeep, draw step or end step, yours,
each or an opponent's (CR 503.1a, 504.2, 513.1a), and a delayed trigger
an instruction creates, "Draw a card at the beginning of the next turn's
upkeep", triggering once (CR 603.7a–c: Portent), an "intervening if"
that counts what its controller controls, checked as it triggers and as
it resolves, "other" than the object its event names when it says so
(Valakut's "if you control at least five other Mountains", CR 603.4), or
echo's "if this permanent came under your control since the beginning of
your last upkeep" (CR 702.30a),
with exalted, prowess, training and echo written
out as the triggers they are (CR 702.83a, 702.108a, 702.149a, 702.30a) and "until end of
turn" effects ending in cleanup (CR 514.2); combat: attackers tapping
unless they have vigilance (CR 508.1f), blockers, the declare blockers
and combat damage steps skipped with no attackers (CR 508.8), a
first-strike damage step for first and double strike (CR 510.4),
combat damage assigned and dealt at once (CR 510.1–510.2), to the player
an unblocked creature attacks or the creatures blocking and blocked,
deathtouch and lifelink as for any damage; a basic land
type's intrinsic mana ability (CR 305.6), which the layers grant after
layer 4, so Yavimaya's Forests tap for {G}, Blood Moon's Mountains for {R}
alone (CR 305.7), and a land that loses all abilities for nothing; "enters
tapped", "unless you control" or "if you control" a number of permanents
a filter, or any of its alternatives, matches ("two or fewer other lands",
"two or more basic lands", "a Mountain or a Plains"), not counting the
land itself, a shock land's life or tapped, and entering with counters
(Gemstone Mine, CR 614.1c, 122.6), all read off the
permanent as it would enter (CR 614.12); mana pools emptying as each step ends (CR 500.5);
a player with hexproof (Leyline of Sanctity's "You have hexproof") never
a legal target of an opponent's spell or ability (CR 702.11c, 115.4),
and one with shroud never anyone's (CR 702.18a); a player's draw step
skipped while a permanent of theirs says so as it would begin (CR
614.10), and all damage that would be dealt to them prevented, a pain
land's too (CR 615.1a: Solitary Confinement);
shroud, and hexproof against an opponent's (CR 702.18a, 702.11b), never
a legal target; indestructible, never destroyed by an instruction or
lethal damage (CR 702.12b); shadow, blocking and blocked only with
shadow (CR 702.28b);
protection from a colour, from multicolored or from everything: never a
legal target of a spell or ability from a source with the quality, never
blocked by a creature with it, and its damage prevented (CR 702.16b,
702.16e–f, 702.16j), never enchanted by an Aura with it (CR 702.16c),
an Equipment never attaching yet;
state-based actions whenever a player would get priority (CR 704.3) for a
token in a zone other than the battlefield ceasing to exist (CR 704.5d,
111.7), 0
life, drawing from an empty library, toughness 0 or less and the legend
rule (CR 704.5a–b, 704.5f, 704.5j), an Aura attached to nothing, to
itself or to what it can't enchant going to its owner's graveyard (CR
704.5m, 303.4c–d), and a commander's return to the
command zone (CR 903.9a); a loser leaving the game with what they own and
a multiplayer game going on without them (CR 800.4a).

A card the game can't run is refused by name when played or cast rather
than run without what it can't do: an instruction with no lowering, named
by its node (`Effect::Destroy`, `Effect::May(Move)`), a triggered ability
on any other event (`Trigger::When(Event::DealsDamageTo)`,
`Trigger::At(Step::Upkeep)`), a reference the ability's binders don't
resolve to a target or its source, a static that isn't
"enters tapped", a mana ability with a cost or effect beyond those above,
`{X}`, Phyrexian or snow mana, a planeswalker or battle (their counters),
an Aura enchanting a player (CR 702.5d) or with two enchant abilities, a
triggered mana ability other than one adding fixed mana to the
controller of a permanent tapped for mana, and a keyword other than
flash, cycling, transmute, enchant, flashback, kicker (a kicker whose cost is more than mana is carried and
never paid; two kickers are refused), morph (a morph cost that isn't
mana is carried and never paid; megamorph is refused), partner, exalted, prowess, training and the layers' own (flying, reach,
menace, first and double strike, defender, haste, vigilance, trample,
deathtouch, lifelink, shroud, hexproof, indestructible, shadow, and
protection from a colour, from multicolored or from everything); cycling
for mana is activated from its owner's hand, discarding it to draw a card
(CR 702.29a), and landcycling and transmute are carried, never
offered. A card is cast for an alternative cost (CR 118.9) rather than
its mana cost from the zone that cost allows: Fireblast's "sacrifice two
Mountains rather than pay this spell's mana cost" from a hand, and
flashback from a graveyard, only an instant or sorcery (CR 702.34a),
the alternative's mana, `{0}` when it has none, changed by cost changes
like any (CR 118.9d), the rest of it (life, sacrificing permanents a
filter matches) paid after the mana (CR 601.2h), offered only when it
can be, and a spell cast for flashback exiled whenever it would leave the
stack; one whose cost isn't mana, life or a sacrifice is carried, never
offered. A non-mana activated ability
the engine can't run (a limit, another cost, another effect) is
carried and never offered, so the card plays and the ability doesn't.

**41 of Loam's 90 distinct cards are played or cast and resolved, 29 of
them with every ability running** (the basics, Birds of Paradise, Elvish
Mystic, Command Tower, Karplusan Forest, Yavimaya, Cradle of Growth, the
fetchlands Misty Rainforest, Prismatic Vista, Scalding Tarn and Wooded
Foothills, the shock lands Breeding Pool, Steam Vents and Stomping
Ground, Nature's Lore, since a search is an instruction like any other,
Fiery Islet and Waterlogged Grove, since an activated ability can cost
mana: "{1}, {T}, Sacrifice this land: Draw a card", and the six more
since cycling runs: Coastal Peak, Desert of the Fervent, Forgotten Cave,
Ketria Triome, Lonely Sandbar and Sheltered Thicket, Deep Analysis,
whose flashback runs, and Lotus Cobra, whose landfall adds mana of a
colour chosen, and Cultivate, Kodama's Reach and Flare of Cultivation,
since a search can find up to two, measured 2026-11-04), Blast Zone played too since it
enters with its counter, its abilities carried:
`cargo test -p chancellor-of-the-spires --test game loam_coverage --
--nocapture` plays each in a game of its own and prints the rest by the
node refused. The most refused: `Trigger::When(Event::Discards)` (4),
then discarding more than a card, `Effect::Move` from a graveyard,
modifying by an amount and playing from another zone (3 each). `tests/game.rs` scripts the rules above, and plays
Loam against a goldfish for six turns, casting what it can.

Not yet: most instructions (modal spells, "if", a predefined token or
one with abilities beyond keywords), X,
activated abilities with limits or costs beyond a tap, life, mana and
sacrificing their source, triggers on any other event (leaving the battlefield other than dying), an "intervening if" other than "if you control …" (CR 603.4), a card that triggers from a
graveyard, attacking a planeswalker or battle, requirements and
restrictions on attacking and blocking beyond flying, reach, menace and
defender, a creature blocking two, an Aura put onto the battlefield
other than by resolving, which enters attached to nothing and so goes to
the graveyard rather than its player choosing what it enchants (CR
303.4f), and a land's back face (CR 712). State-based actions run one creature after the other rather than at
once (CR 704.3), so of two creatures dying together, the one put into
the graveyard second sees the first die but not the other way round (CR
603.10a).

## What the layers cover

A static "as long as you control a Mountain" or "as long as it's your
turn" exists only while its condition holds, read from the battlefield
as the layers have made it so far (CR 611.3a), so a land Blood Moon makes
a Mountain in layer 4 grows Wild Nacatl in 7c, and an effect that would
change whether the condition holds is one it depends on (CR 613.8a).
Layers 4 (adding types and subtypes, setting a basic land type, CR 305.7),
5 (colour), 6 (granting, removing, removing all) and 7b–7d (setting,
modifying, switching), counters in 7c, filters over type, supertype,
subtype, colour, controller, "nontoken", "other", "this" and "enchanted",
and alternatives of one type, supertype or colour each ("artifact or
enchantment").
`tests/layers.rs` has CR 613.4d, 613.5, 613.6 and 613.9's worked
examples, built by hand since their cards are hypothetical. `tests/oracle.rs`
reads real cards from their Oracle text: Glorious Anthem with counters,
Giant Growth under Humility, Inspired Charge's fixed set against Glorious
Anthem (CR 611.2c), Blood Moon with Urborg and Humility with Opalescence in
both timestamp orders against their Gatherer rulings, March of the Machines
(CR 613.6, 205.1b), Crusade, and Humility with Toph, the First Metalbender,
including Toph's own earthbend (CR 701.66a) on a Forest and on a Sol Ring
Toph made a land.

Layer 1 is each object's copiable values, the game's to write as a copy
effect begins (above, *The game*).

Not yet: a copy effect that ends, layers 2–3 (control, text), 7a (characteristic-defining
abilities, and so CDAs first, CR 613.3), dependency through "what it does"
(CR 613.8a), counters' own timestamps (CR 613.7c), abilities granted with a
later timestamp (CR 613.7a), effects on players and rules (CR 613.10–11),
and conditional statics other than "as long as you control …" and "as
long as it's your turn" (life totals, cards in hand, "if").

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

**531 of the 574 cards in the Modern metagame lower whole** (557 of 600
faces), measured 2026-11-12 against the Scryfall bulk file:
`cargo test --release -p chancellor-of-the-spires --test coverage -- --ignored --nocapture`
prints the number and the refusals by node. It was 528 until a card
entering as a copy ("You may have this land enter tapped as a copy of
any land on the battlefield") was carried for the state machine, and 526
until "as this
enters, choose a creature type" was carried for the state machine, its
"the chosen type" read off the permanent, and "your opponents control"
lowered, and 524 until a rule that
holds only during its controller's turn ("During your turn, spells you
cast cost {1} less", Temur Battlecrier's kind) was carried
for the state machine rather than refused, and 523 until the layers
named first and double strike, menace, reach, defender and protection, so
a static granting one lowers, and 522 until "as long as you control …" and "as long as it's your
turn" lowered with a static over its own object, and 495 until a shock
land's "As this land enters, you may pay 2 life" (19 cards) and a static
"if" over rules alone ("If you control two or more other lands, this land
enters tapped", 8) were carried, since neither changes a characteristic.
The most refused are modifying by an amount (8), "as this enters" (5, other choices, or a copiable effect if
it sets a size, CR 613.2a), "as long as" over an amount (4) and 7a's
characteristic-defining abilities (`SetPt`, 3). Karn Liberated and two others don't parse:
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

`chancellor-logs replay [--top N] [--game ID] [--finished F] [--lines F]` plays the cached games
through the engine with `replay::replay`: a two-player constructed game,
20 life, the starting player from "chooses to play first" (or a skipped
first draw, or turn 1's player), the London mulligans the log shows, and
each library **60 hidden cards** (the log never says the deck size; 60 is
constructed's minimum, CR 100.2a). A card is named as it first shows up in
a hand: played, cast or discarded. Then each line becomes answers: priority
passed until the turn and step the line can happen in (the engine's turn
has to be the log's, counting each player's turns), a land played, a spell
cast with its payment found by searching the mana abilities the engine
offers (fewest first, the least flexible sources tapped first, a
filter land or Signet after the sources whose mana it spends, and an
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
shows the player playing anywhere in the match (their deck has it), then
each their opponent plays, then each card it shows them casting or
playing later in the game (a tutor's find shows up again), and last, as
a hypothesis, a real land the log never names at all: one of the Scryfall
bulk file's lands legal in the game's format that the search's filter
matches, those making the colours the player's spells need most first,
at most eight. Only Modern has a legality the bulk file gives (a
Freeform match whose comment or event says Modern); Penny Dreadful's
isn't in it, so its games and other Freeform ones get no such land. The
engine refuses any find that doesn't match what was searched for, the
hypothesis included; and if a later spell can't be paid for, it goes
back and finds the next. The report counts the games whose replay, as it
ended, relies on a land the log never names, apart from games to the
end: the way it reports, the one that got furthest, not the last it
tried (which counted 7 more games until 2026-11-12). A shock
land's life is the same: no line says it was paid and life totals aren't
printed, so the replay doesn't pay and the land enters tapped, unless that
leaves a later spell unpaid for, when it goes back and pays. Life is paid
only when something that follows needs the land untapped. A creature
type chosen as a permanent enters is the log's "chooses Beast for
[Engineered Plague]" line, read ahead to the end of the turn. The game it
replays is one consistent with the log, not necessarily the one played.

The cards the log puts onto the battlefield before the first turn
("puts [Leyline of Sanctity] onto the battlefield") are named in the
hand each player keeps, as they keep it, and begin the game there (CR
103.6a); a mulligan's bottomed cards are others.
An activation line names the card, not which of its abilities: the
replay takes the one whose Oracle text shares the most words with the
text the line prints of it (Mishra's Factory's "becomes a 2/2" or its
"+1/+1"), and stops if two tie. A play never offered goes back to an
earlier payment as a spell no land pays for does, since a permanent
tapped for mana can't then block or tap again. A spell's targets are the log's: a player by their seat, a card by a
permanent of that name among the legal targets; when two share the name,
which nothing the log says tells apart, an opponent's first (most spells
aimed at a permanent are removal: Naturalize on one of two Mox Diamonds),
the caster's own first for an Aura, and the other if a later play can't
be made. A hand revealed to
Duress or Thoughtseize names its hidden cards: the log prints the hand,
after the discard in older logs and all of it in newer ones, and the card
discarded, and the engine's hand must have as many cards or the replay
stops (`HandSize`). The cards a scry looks at are hidden alike, so the
first as many as the log says go back on top and the rest to the bottom.
A card revealed from a library (Dark Confidant, Goblin Guide) is named
by the log's "reveals [X] with [Y]'s ability" line for it, read ahead
from the line being followed to the end of the turn, and the replay
stops if there is none. A card only looked at (Impulse, Mirri's Guile)
stays hidden: the log never names it, so the replay takes the first of
those looked at as the one chosen, a hidden card in the hand until a line
plays it, and puts the rest back in the order they were; its place is
the engine's, so the library's order stays honest though its names are
unknown. A "put a card on the bottom of the library" line must match a
card the engine put on the bottom that turn, and a "creates three
Goblin Tokens" line as many tokens the engine created.
A "may" is answered by its own line when the log prints one ("chooses
to use", "chooses not to use" or "declines to use [X]'s ability") from
the line being followed to the end of the turn; with none, the replay
follows it and keeps a checkpoint to go back and decline it if any later
line can't then be followed, since the log either shows what the
instruction did or nothing at all. Goblin Lackey's is the second: the card
it puts onto the battlefield is never named as it happens, so the replay
names the first card of the player's a later line shows on the
battlefield (a trigger's source, an ability activated, an attacker) more
times than they have permanents of it and lines have cast or played it
since (a second Goblin Warchief attacking beside one cast after the
trigger); with none, it declines.
A triggered ability goes on the stack because the rules put it there,
never because a line says so, and each "puts triggered ability from [X]
onto the stack" line is matched to one of the engine's: a trigger the
engine puts on the stack that no line shows, or one a line shows that the
engine never triggered, stops the replay (`UnloggedTrigger`,
`Untriggered`), and a trigger's targets and the order of several are read
from the lines that show them. A draw "with" a spell or ability is the
engine's own as it resolves. An attack is the log's "is being attacked
by" line, its attackers by name, and the blocks the "[X] blocks [Y]"
lines after it, the first of two creatures of one name where the log
can't tell them apart; a turn whose lines show no attack declares none.
How a creature blocked by two divides its damage is never printed, so
the replay assigns each blocker lethal damage in turn and the rest to the
last, or past them to the player with trample. The log doesn't say when
a spell resolved either: the stack resolves before a line's play,
down to what a later line shows resolving ("reveals their hand to
[Duress]"), so a fetchland activated in response to Duress is activated
with Duress on the stack. A spell that targets a permanent a spell on the
stack targets is cast in response to it too, and so is any spell whose
casting after the stack resolved leaves a trigger the log doesn't show,
or misses one it does (Warmth's on an opponent's red spell, cast in
response to Warmth): the replay goes back and casts it so. A "removed from the
stack because it has no legal targets" line must match the engine's
spell leaving the stack unresolved (CR 608.2b).

A cast line says how the spell was cast when it wasn't plainly from
hand for its mana cost ("from the graveyard", "with Flashback{1U}, Pay 3
life from the graveyard", "by sacrificing 2 Mountains"), and whether it
was kicked ("with kicker", "(with kicker)", beside any of those), and
the replay casts it that way, its kicker paid only when the line says so, by the card's alternative cost from that zone; older
logs print nothing for Fireblast's sacrifice, so a line that says nothing
is cast for the mana cost and, if that can't be paid, for an
alternative cost from a hand. The permanents sacrificed to pay are never
named: the replay sacrifices first a creature whose dies trigger the lines
right after show, then one no later line names, and goes back to another
if a later line can't be followed. The card Cabal Therapy names is the
log's "names [X] for [Cabal Therapy]" line, the hand revealed to it names
the hidden cards of it, and each "discards" line after must match a card
the engine discarded by that instruction. An ability of a permanent a
spell or ability on the stack targets is activated in response to it
(Mogg Fanatic sacrificed to Seal of Fire's damage), and only an ability
that can target a spell is aimed at one; of permanents of one name, the
replay activates one not yet activated this turn (a second Mishra's
Factory animated, not the first again), and goes back to another if a
later play can't be made; a payment taps last what a later line this
turn attacks, blocks or activates with, and sacrifices for mana last of
all; a "puts a -0/-1 counter on [Wall of Roots]" line is its mana
ability activated then, the mana floating; a spell its caster casts in
their own upkeep or draw step with nothing to answer is cast in their
main phase instead (Dark Ritual's mana would empty first), earlier only
if what follows needs it; mana of the wrong colour in the pool is no
reason to stop tapping lands, and a payment may run a filter land on
mana it leaves over, the rest floating for a later spell, tried only
when a later spell can't be paid otherwise (Flooded Grove's {G}{G}
paying two Fyndhorn Elves); a City of Brass trigger line with nothing on
the stack is the land tapped for mana then, in its controller's main
phase if it is their turn; a counters line resolves the stack only until
the counters it shows are there, each line's counted apart from those
before; a split card's halves listed beside it in a revealed hand are
one card; an "unless" the log shows paid only for echo ("paid the echo
cost for [X]"), so echo is paid when that line follows and not
otherwise, and anything else, Mana Leak's {3}, isn't paid unless a later
line can't be followed then; a discard paying an "unless" is the
player's next "discards [X]" line that turn, and with none it isn't
paid; Phyrexian Dreadnought's sacrifice is never paid, which no log
shows; a "skips their draw step." line past the first turn is a
permanent's skip the engine made too (CR 614.10); a payment never activates a mana ability
whose counter the log would print (Wall of Roots); a "draws two cards
with [Sylvan Library]'s ability" line is its "may" taken, the "kept a
card." and "put a card on top of …'s library." lines after it answer
each card's "pay 4 life or" in the order printed, and since the cards
drawn are hidden and the log never says which two were chosen, the
replay chooses the first two drawn; a "reveals [X] from their opening
hand." line before the first turn names X in the hand kept, as a
Leyline's line does, and reveals it; a "reveals [X]" line names the card
only of a search whose instruction reveals what it finds, and one that
doesn't (Viridian Emissary's, resolving first) is answered as a
fetchland's is; which of the lands Flare of Cultivation found goes onto
the battlefield is never printed, so the replay puts the first revealed
there and goes back to the other if a later line can't be followed, as
for any choice among named cards the log doesn't show; and mana a line shows
made for a sorcery-speed spell in its caster's turn is made in a main
phase, the other player passing to its controller first when it is
theirs to act (a Wall of Roots tapped for mana while blocking); a
"removes a mining counter from [Gemstone Mine]" line is its mana
ability, the colour the next spell asks first; two block lines naming attackers of one
name block two of them; the legend rule's choice, which the log never
shows, keeps the first and goes back to another if a later line can't be
followed; and a spell whose casting after the stack resolves leaves a
choice the log doesn't show (Duress's) is cast in response, as one that
leaves an unshown trigger is. A "returns [Island] and [Island] to its
owner's hand with [Gush]'s ability" line beside a line casting Gush, before
or after it, is Gush cast for its alternative cost, even when the cast
line doesn't say so, the permanents returned those it names, tapped ones
first; since the log never shows a mana ability, those not tapped already
are tapped for mana before they go, the mana floating for what is cast
next, as a land returned untapped would have made nothing. Any other
"returns … to its owner's hand" line must match as many permanents the
engine returned to a hand that turn, each in a hand now. A "discards [X]"
line just before its player activates an ability whose cost discards
(Survival of the Fittest) is that cost, which isn't followed yet, never a
resolving instruction's discard; and a spell cast in its caster's combat
with nothing to answer is cast in their postcombat main phase instead if
what follows needs it (the Dark Ritual for the Phyrexian Arena cast
next), as one cast in the beginning phase is cast in the first.

A line names a two-faced card by the face it shows: "plays [Hagra
Broodpit]" is Hagra Mauling // Hagra Broodpit played by its land face,
"casts [Steam Clean]" Scalding Viper cast as its Adventure, "is being
attacked by [Scalding Viper]" the permanent whichever face's name a line
uses. A cast of an adventurer card whose player has one on an adventure
is of the one in exile, which the line never says, before one in hand;
and a return-to-hand line beside a cast is the cast's cost, as Gush's
is, only for a card with an alternative cost from a hand, Steam Clean's
being its own instruction. Delver of Secrets's "may" is taken when a
"reveals [Lightning Bolt]." line, which says nothing of what revealed
it, names the card it looked at, and declined otherwise; a "[Delver of
Secrets] transforms into [Insectile Aberration]." line resolves the
stack until the engine has transformed one more permanent that turn,
which must have that face up.

What a card entering as a copy copied is never printed either: the log
shows only what the copy does after, by the name it then has ("targeting
[Cloudpost]" for a Vesuva that copied one, "returns [Vesuva] to its
owner's hand" once it has left). So the replay tries one permanent of
each name it may copy, those a later line of the game names first, then
its player's own, then entering as itself, and goes back to the next if
any later line can't be followed: a Vesuva that copied a Forest leaves
too little mana for a later spell that two Cloudposts pay for.

A card cast face down ("casts a card face down.", "Cast face-down with
Morph.") is one the log hides, and each is "face-down creature #N"
after, numbered by the casts. A later line names it: "reveals face-down
creature #1 as Exalted Angel." as it leaves the battlefield or the stack
(CR 708.9), or, once a "turns face-down creature #1 face up." line, the
first card with morph a line after that names (the trigger from [Nantuko
Vigilante], an attack by [Exalted Angel]). The replay reads that ahead
and casts that card of the hand face down, a hidden one named as it; with
no line naming it, a hidden card is cast face down unnamed, the 2/2
being the same whatever it is. Targets, attackers and blockers named
"face-down creature #N" are that permanent, which no line naming a card
matches while it is face down; the reveal line resolves the stack until
it has left; and the turn face up line is the special action, taken in
response to what is on the stack.

The log stops printing as the game ends, so what was on the stack and
ended it is never shown resolving (the second of two Razorkin Needlehead
triggers on a draw): a "wins the game" line resolves the stack, passing
as a line with nothing to show does, and the game replays to the end
when that ends it with the log's winner (CR 104.2a), and diverges
(`LogOver`) when it doesn't; an attack the log shows last deals its
combat damage first.

The first line the engine won't follow ends the game's replay with a
typed `Stop`: a card refused by the node of its text the engine can't run,
an answer it calls illegal, a state that disagrees with the log (a play it
never offers that turn, a spell it can't pay for, a turn out of step), or
a kind of line not followed yet (an exile, a creature put onto the
battlefield). `--game ID` prints one game line by line with what the engine
did and each player's life as it ended; `--finished F` writes the ids of the games replayed to the end, to
diff against a run before a change for a finished game it broke, and
`--lines F` each game's lines followed, its lines, its stop and the lands
the log never names it relied on, one game a row, to diff for a game a
change made follow fewer; the report ranks stops by games, with an example line, players
renamed P1 and P2.

**Of 1,061 games replayed, 72 replay to the end** (69 to the log's winner
line), 54 of them past the first turn, and **30.14% of their lines are
followed** (23,565 of 78,183: the lines before each game's stop, all of a
finished game's, a measure that moves before a game's last stop goes),
measured 2026-11-12, and 8 games rely on a land the log never names (1
of them to the end); it was 71 games and 30.05% before copies, 29.95%
before transforming cards, 70 games
and 29.56% before modal double-faced and adventurer cards, 65 games and 29.39% before the "unless" costs
other than mana, 57 games and 28.75% before morph, 20 games and 20.96% before the milestone's
work (below), and 15 games before spells' effects, triggers and combat
ran. They are the
1,299 games that parse whole and that Insight reads whole, less 238 with
no turn in them (empty logs, concessions during the mulligans). 283 stop
in the first turn, 341 in the second, 194 in the third, 118 later, and 53
at the setup (21 logs name one player, 6 begin mid-game, 3 aren't London
mulligans, 2 disagree on who started, the rest mostly a card begun on
the battlefield that the engine refuses: Leyline Axe, Leyline of the
Void, Gemstone Caverns, Quicksilver). Lands stop few games now: fetchlands, shock, fast, battle and
check lands all run, Cloudpost's "for each Locus on the battlefield" too,
and the most a land stops is Fabled Passage's "then if you control four
or more lands, untap that land" (12). A refused spell or trigger is
ranked by the node it needs. From 19 games and 19.07% of lines on
2026-11-02 (morning), each step's numbers were: Cloudpost's count 19.40%;
"may" and Goblin Lackey 19.67%; upkeep, draw and end step triggers with
Aether Vial 19.83%; revealing and looking at a library's top cards 20
games, 20.43%; Leylines 20.55%; "as long as" 20.62%; destroy, exile,
bounce, counter and dying 20.72%; search as an instruction 20.81%;
responses 20.84%; tokens 20.96%; then the milestone's work took it to
23.99% and on to 27.27% (below). Games to the end barely moved: a game
that gets past one stop meets the next, most in its first two turns. The
stops now, by games: an exile line not followed (76: suspend's Rift Bolt,
Grim Lavamancer's cost), `Trigger::When(Event::Or)` (43: Stitcher's
Supplier), a restriction (30), modifying by an amount (26), looking at a
hand (23: Gitaxian Probe), modal spells and Cavern of Souls's "spend
this mana only" (22 each), "with" (21), one player named (21), and
moving cards a search found (20). Of the 55 games two faces stopped, one
replays to the end and 32 stop at other cards; 15 stop at a
transforming card's other text (Invasion of Ixalan's "With" in 4, Thing
in the Ice's "remove an ice counter", Search for Azcanta's surveil, a
back face refused: Ashling, Revealing Eye) and 7 at a split or prepare
card. None is the engine disagreeing with Magic Online; those that
were are fixed: Gaea's Cradle's "Add {G} for each creature" not read as a
mana ability (CR 605.1a), and Champion of the Parish's "another Human"
missing a Human, since a creature type said with no card type was read
as no creature's (CR 205.3m), and Cloudpost's "Locus", a land type said
with no card type, read as no land's (CR 205.3i), which counted no Locus
once "on the battlefield" lowered. 10 games stop on a spell no land pays for
after a fetchland found what the replay chose, where the land it found
was one the log never shows played (a Watery Grave from Polluted Delta).
`tests/replay.rs` replays hand-written logs, a short game to its end and
the plays that must stop it: a second land, a spell no land pays for, a
land in the opponent's turn; a fetchland whose search the replay answers
again when the first land it found can't pay for a later spell, and one
the log says found nothing; a shock land paid for only when a spell
that turn needs it; and Duress's revealed hand and Champion of the
Parish's trigger, which must each match the engine's; an attack and
a block; Goblin Lackey's "may" read from the lines after it, Aether
Vial's upkeep trigger, a Leyline from the opening hand, and a Swords to
Plowshares cast in response to a Lightning Bolt at the same creature;
and a Gush whose return line comes before a cast line that says nothing
of its cost, its Islands' floating mana paying for what follows; and two
cards cast face down, one named by the line revealing it as it dies, the
other by the Exalted Angel attacking once a line turned it face up; and
a Solitary Confinement kept by a discard line, its controller's draw
step skipped, then sacrificed with none; and Pelakka Caverns played by
its face's name, Steam Clean cast as an Adventure and the Scalding Viper
cast after it from exile; and Delver of Secrets declining to reveal,
then revealing a Lightning Bolt and attacking as Insectile Aberration;
and a Vesuva gone back to and copied as Cloudpost when its copy of a
Forest can't pay for two Bronze Sables. `tests/effects.rs` runs Thoughtseize, Lightning Bolt (at a
player, at a creature, and at one already gone), Preordain, Champion of
the Parish, Temple of Deceit, and combat: an unblocked attacker,
flying against a block, a trade, first strike, exalted, prowess,
Scorch Spitter's attack trigger and Jackal Pup's damage trigger;
Cloudpost, Goblin Lackey's "may" taken and declined, Aether Vial, Dark
Confidant, Goblin Guide, Impulse, Leyline of Sanctity's hexproof, Wild
Nacatl, Swords to Plowshares, Counterspell, Festering Goblin's dies
trigger, Path to Exile, Khalni Garden's token, Silver Knight's
protection from red, Goblin Warchief's and Thalia's cost changes, and
Sylvan Library keeping one card for 4 life and putting the other back,
Bonesplitter equipped only at sorcery speed, staying unattached when
its creature dies, Chancellor of the Forge's Goblin at the first
upkeep, Flare of Cultivation cast by sacrificing Llanowar Elves, and
Accumulated Knowledge drawing one more for each in a graveyard, and
Nimble Mongoose growing from the seventh card in its graveyard, and Gush
returning two Islands rather than paying its mana cost, and Engineered
Plague and Plague Engineer shrinking the creature type chosen as they
enter, and Exalted Angel cast face down as a 2/2 and turned face up for
its morph cost, gaining the life it deals, and Nantuko Vigilante turned
face up destroying an artifact, and Masticore kept by a discard or
sacrificed, Phyrexian Dreadnought sacrificed unless twelve power is,
and Solitary Confinement skipping its controller's draw step, giving
them shroud and preventing a Jackal Pup's damage to them, and Pelakka
Predation // Pelakka Caverns played by its land face, a land alone on the
battlefield, and cast by its sorcery face, and Steam Clean cast as an
Adventure, bouncing Grizzly Bears into exile and the Scalding Viper cast
from there, and Delver of Secrets transforming into a 3/2 of mana value
1 when the card it reveals is an instant and staying a 1/1 otherwise,
and Vesuva entering tapped as a Cloudpost, a Locus each Cloudpost
counts, or untapped as itself, and as a Glimmerpost gaining the life
its trigger counts, and Thespian's Stage becoming a Cloudpost that keeps
its copy ability.

### The milestone

General features barely move games to the end: the 6,648 games use 4,008
distinct cards, a median of 19 a game and 630 seen once, and a game
finishes only when all of its cards work. Insight's lesson holds here too:
open-ended coverage plateaus, a fixed card pool is doable. So the replay
has a milestone, worked card by card as Insight's round 9 was.

`chancellor-logs milestone --games 500` takes the games the replay plays,
less those that stop at the setup (the log's shape, one player named or a
game begun late, which no work on the engine changes), and chooses 500
greedily: each time the game needing the fewest cards not yet in the
pool, the lowest id on a tie. It writes
[`milestones/replay-500.txt`](milestones/replay-500.txt), Magic Online's
numeric game ids and the sorted card pool, nothing of the logs. `replay`
then reports each milestone apart: its games to the end, its lines
followed, and its stops ranked by games, each naming the card and node.

**replay-500 is 500 games and 804 cards.** At its writing
(2026-11-02) **20 of them replayed to the end and 24.53% of their lines
were followed** (7,766 of 31,659); the top of its worklist was Mox
Diamond's replacement effect (19 games), Fanatic of Rhonas's "only if"
mana ability, Hired Claw's attack trigger and Portent's delayed draw (15
each), and Cabal Therapy's named card (13).

Worked card by card from there, **on 2026-11-12 70 of the 500 replay to
the end and 43.69% of their lines are followed** (13,832 of 31,659), 2
of them relying on a land the log never names (1 to the end), and 83
games have no card the engine refuses before any game (from 21). The
general rules each card needed, in the order the worklist asked:
protection and the keywords the layers name; cost changes (Thalia, Goblin
Warchief); "for each" sizes counted on resolution (Goblin Piledriver);
activated abilities with targets and mana costs (Mogg Fanatic, Rishadan
Port); a land becoming a creature until end of turn (Mishra's Factory);
mana abilities that cost mana (filter lands, Signets) or count what you
control (Fanatic of Rhonas); triggers on anyone's spell (Eidolon of the
Great Revel); shroud, hexproof, indestructible and shadow; mana added by
a spell (Dark Ritual); instructions on each permanent a filter matches
(Thalia's Lieutenant, Pyroclasm, Wrath of God); cycling; attacking with
a creature type (Hired Claw); delayed triggers (Portent); a discard
instead of entering (Mox Diamond); revealed cards a filter sorts (Goblin
Ringleader); alternative costs and flashback, a card name chosen and a
hand revealed (Cabal Therapy); training (Hopeful Initiate); an
intervening "if" (Valakut); "then if you control" (Fabled Passage); draw
triggers (Razorkin Needlehead); mana abilities costing a counter, once
each turn (Wall of Roots), or a sacrifice (Skirk Prospector); damage a
source you control deals an opponent, a spell's too (Quest for Pure
Flame); the beginning of combat (Luminarch Aspirant); becoming tapped
(City of Brass); mana of a colour chosen as an instruction resolves
(Outcaster Trailblazer); "unless" paid or not (Mana Leak, echo); a
cycling trigger and an X counted (Gempalm Incinerator); entering with
counters and a mana ability taking one off (Gemstone Mine); another land
a turn (Exploration); an activated ability limited to once a turn and
to an opponent having lost life (Hired Claw); a cost change for each
permanent during its controller's turn (Temur Battlecrier); "for each
land card discarded this way" (Gerrard's Verdict); kicker and "if this
spell was kicked", with Insight reading "it deals 4 damage instead" as
the spell's damage, not its target's (Burst Lightning); Auras cast
targeting what they enchant, attached as they resolve and put into the
graveyard when what they enchant is gone, and a triggered mana ability
(Wild Growth); a draw step "may" drawing more, a choice of the cards
drawn this turn and "for each of those cards, pay 4 life or" put it
back, with Insight reading "the card" as the loop's one card (Sylvan
Library); equip, an ability activated only as a sorcery, attaching its
Equipment, which comes off a creature gone or no longer one (Leyline
Axe); a card revealed from the opening hand for a delayed trigger at the
first upkeep, and tokens counted as they are made (Chancellor of the
Forge); a search for up to two, none finding what one before found
(Flare of Cultivation); a number of cards drawn counted as it
resolves, of cards in graveyards by name (Accumulated Knowledge), and
threshold's "as long as there are seven or more cards in your
graveyard", the layers reading each graveyard's size (Nimble Mongoose);
returning permanents to hand as an alternative cost (Gush); a creature
type chosen as a permanent enters, which "the chosen type" in its
abilities reads, and "your opponents control" (Engineered Plague, Plague
Engineer); morph, cast face down as a nameless 2/2 and turned face up as
a special action, "whenever this creature deals damage" (Exalted Angel),
"when this creature is turned face up" and a target that is any of
several types, "artifact or enchantment" (Nantuko Vigilante, which
Naturalize and Disenchant read through too); an "unless" paid by
discarding a card or sacrificing creatures of a total power (Masticore,
Phyrexian Dreadnought), and a player's draw step skipped, their shroud
and all damage to them prevented (Solitary Confinement); a card's faces
read apart, the one up the card's state, a modal double-faced card
played or cast by either (Hagra Broodpit, Pelakka Caverns), an Adventure
exiled as it resolves and its creature cast from exile, and "return
target nonland permanent" (Scalding Viper // Steam Clean); transforming,
an ability on the face it is printed on, and a card looked at then
revealed (Delver of Secrets); a copy's copiable values as layer 1,
entering as a copy and becoming one (Vesuva, Thespian's Stage), and "1
life for each Locus" (Glimmerpost). And
the replay's inferences: which of a card's abilities a
line activates, a spell cast in response when the triggers after say so,
a search's card from the line that reveals it, a shuffle matched to an
instruction's, the creature sacrificed for flashback, a land the log never
names as a hypothesis, which of two permanents of one name an
activation is of, a winner line resolving what ended the game, and
which of the cards drawn this turn Sylvan Library chooses, which
land Flare of Cultivation puts onto the battlefield, and which card was
cast face down, from the line revealing it or the card with morph a line
names once it is turned face up, the discard paying an "unless"
from the next discard line, and which face of a two-faced card a line
names and which copy of an adventurer card it casts, and what Vesuva
entered as a copy of. One divergence was the engine's own, found by the full
run: a resolved effect's fixed set held a creature that had left the
battlefield, and the layers panicked reading it (CR 400.7, fixed). The
top of the worklist now: Stifle countering Phyrexian Dreadnought's
trigger (9: six of the Dreadnought games stop there), and Grim
Lavamancer's exile line, Goblin Cadets, Quicksilver and Veteran Survivor
(6 each); of the 8 games Vesuva stopped and the 2 Thespian's Stage did,
one of the Stage's replays to the end and the rest stop at other cards
(Harmonic Prodigy's additional trigger, Pithing Needle's card name,
Search for Tomorrow's suspend, Trading Post's counters, Price of
Progress's "each player" among them); of the 34 games two faces stopped, Hagra
Broodpit's and Scalding Viper's among them, one replays to the end and
18 stop at other cards (Stomp, Lizard Blades, Ayara), 10 at a
transforming card's other text (Invasion of Ixalan's "With" in 4) and 5
at a split or prepare card; of the
Solitary Confinement and Masticore games, five now replay to the end
and the rest stop at other cards (Elephant Grass's cumulative upkeep,
Replenish, Goblin King, Eternal Dragon's landcycling). No
divergence is left: those the run found were the replay's (the game's
last trigger never shown resolving, a payment search that stopped adding
sources once it had as much mana as the cost, of the wrong colour from
Wild Growth's Forest, a block or
an activation of the wrong one of two permanents of one name, a ritual's
mana cast away in the draw step, a filter land's floating mana never
tried, a fetchland's find behind more than the 64 later choices the
replay went back through, now 512; and, once morph and "artifact or
enchantment" let games run further, Goblin Lackey's card taken for one
cast because another of its name was, Naturalize aimed at its caster's
own Mox Diamond rather than its opponent's, a winner line after an
attack that never dealt its combat damage, a Dark Ritual cast in combat
for a Phyrexian Arena that waits for the second main phase, and the
discard paying Survival of the Fittest taken for Gerrard's Verdict's,
now a stop of its own; and, once Masticore let a game run on, a Goblin
Lackey "may" guessed taken reported as a divergence where declining it
reached the same line and stopped at Eternal Dragon's landcycling, now
the stop reported, since a line not followed yet says more than a
guess's divergence there), each fixed; and one was the engine's
reading, Cavern of Souls's mana ability, whose "spend this mana only"
binds a description and no target, taken for an ability that isn't a
mana ability and carried, where now the card is refused by name (CR
605.1a).

## Speed

On the research's game-scale board (300 objects; two layer 4 effects, 14
setters and 14 anthems), one from-scratch run takes **1.1 ms** natively
(release, `timing_on_the_research_board`, ignored by default). The research's
toy took 0.34–0.51 ms but never re-decided its order after each effect.
Layer 7 skips the dependency check, since nothing a filter reads changes
there; without that the same board took 53 ms. Not measured in wasm yet.
