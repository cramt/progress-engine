# Semantics log

What a tree means where the text leaves room. Each grammar round adds the
calls it made here, and a later round follows them instead of guessing again.
A call that the Comprehensive Rules settle cites the rule. An open call says
so, and stays open until someone decides it.

Status: **settled** (follows the CR or a deliberate decision), **open**
(the grammar does something, and nobody has checked it is right).

## Triggers

| Text | Tree | Status |
|---|---|---|
| "When you do, …" | `ReflexiveTrigger`, not `IfYouDo` | settled: CR 603.12 |
| "When/Whenever X, if Y, Z" | intervening-if on the trigger, not `Effect::If` | settled: CR 603.4 |
| "When X and whenever Y" | one trigger on either event, the same tree as "Whenever X or Y" | settled: CR 603.1b, one ability with two trigger conditions |
| "This ability triggers only once each turn" | a limit on the trigger | open |

## Objects and players

| Text | Tree | Status |
|---|---|---|
| "any creature" | `Quant::One`, the same as "a creature" | settled: one, of the chooser's pick, either way |
| "a second target" | `SecondTarget`, not `AnotherTarget` | settled: CR 115.3, a second "target" may choose the first one again; "another" may not |
| "the other" (of two) | `Quant::Rest`, what the earlier choice left, not "it" | settled |
| "this creature or another creature" | `SelfOrAnother` keeps "this creature", as `SelfRef` does elsewhere | settled: the same object as "~", kept because the type line is not in the tree |
| "that player", "they", "the player" | `Player::That` | settled: one back-reference |
| "each opponent", "your opponents" | `EachOpponent` | settled: CR 102.2–102.3, the same players |
| "up to one target player or planeswalker" | an object whose kind is Player, not a separate player alternative | open |
| "each with power …" | the condition holds for every one; "each" is dropped | settled: "each" restates what a filter says of every object |
| "with lesser mana value" | compared against the object the ability triggered on, which the text leaves unstated | open |

## Zones

| Text | Tree | Status |
|---|---|---|
| "put … on top" or "put them back" with no library named | `OwnersLibraryTop` | settled: the library just named, which is the owner's (CR 400.3); "back" is the top they were looked at from |
| a bare "the top card" or "on the bottom" | the owner's library | settled: CR 400.3 |
| "is put into exile" with no "from" | `from: Zone::Anywhere` | settled: an event that names no zone comes from any |
| "put", "return", "put … back" | one `Move`, no verb in the tree | settled: no rule reads the verb (CR 400.6 names a move by its zones) |
| "doesn't untap during your untap step", "… its controller's untap step" | `DoesntUntap(Player)`, whose untap step | settled: for an Aura on another player's creature they are different steps |
| "the next end step", "the next turn's upkeep" | `Whose::Next`, `Whose::NextTurns`, not `The` | settled: "the next" may be a later turn's |
| "first main phase" | `PrecombatMain` | settled: CR 505.1a |
| "second main phase" | `SecondMain`, apart from "postcombat main phase" | settled: CR 505.1b counts main phases |

## Effects

| Text | Tree | Status |
|---|---|---|
| "protection from each color" | protection from each of the five colors | settled: CR 105.1 |
| "Otherwise, …" | `Otherwise`, the other branch of the "if" before it | settled: no option was offered, so it is no "If you don't" |
| "It's an enchantment." after returning a card | `IsNow` | settled: CR 611.2e, it is that as it enters, which "becomes" is not |
| "that many plus one +1/+1 counters are put on it" | `CountersArePut`, said in the passive | settled: put by whoever was putting the counters, not by "you" |
| "that color", "of that type" | `Palette::That`, `OfThatType`, apart from "the chosen color/type" | settled: "that" binds to the nearest one named, which inside "for each color" is the pass's color |
| "that damage" | `Amount::ThatDamage`, apart from "the damage dealt this way" | settled: the event's damage, not this ability's |
| "any number of target creatures" | `AnyNumberOfTarget`, apart from "up to X target" | settled: no bound |
| "the next spell you cast this turn" | `WillCastThisTurn`, apart from "spells you've cast this turn" | settled |
| "any of that mana is spent on" | `AnyManaSpentOn`, apart from "you spend this mana to cast" | settled: some of it, not all |
| "cards revealed this way" | `Marked(Revealed)`, not "from among them" | settled |
| "Equip legendary creature {3}" | `CostFor`, the objects and the cost | settled |
| "choose artifact, creature, enchantment, instant, or sorcery" | `Choice::CardTypeAmong` | settled: a card type, not objects |
| "has base power and toughness X/X" | `Becomes` with that size | settled: CR 613.4b, the same layer as "becomes X/X" |
| "Each player shuffles …, then draws seven cards" | the draw continues "each player" | settled: a third-person verb continues the subject before it |
| "choose both" | `Choose::N(2)` | open |
| "one or more" | `AtLeast(N(1))` | open |
| "unless <condition>" | `If { Not(condition) }` | settled: "unless" is "if not" |
| "… instead" after an effect, even on the previous line | `Effect::Instead` replaces the effect before it | open |
| "Repeat this process until …" | `RepeatUntil`, which repeats everything before it in the ability; nothing in the tree says how far back | open: the tree should say |
| ", whichever comes first" | consumed with no node, because "until A or B" already ends at the first | settled |
| "Do this only once each turn" | a limit on the whole ability | open |
| "if it had counters", "if it was a creature" | `Condition::Past`, as it was when the event happened | settled: last known information (CR 603.10a for leaving the battlefield), not as it is now |

## Durations

| Text | Tree | Status |
|---|---|---|
| a duration inside a `During` block | defaults to `Permanent` | open |

## Lexing

| Text | Tree | Status |
|---|---|---|
| any parenthesised text | dropped as reminder text, which loses rules text such as Mistform Ultimus's "(even if this card isn't on the battlefield)" | open: should keep parentheticals that are not reminder text |
