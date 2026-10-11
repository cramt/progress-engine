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
| "When X and whenever Y" | one trigger on either event | open |
| "This ability triggers only once each turn" | a limit on the trigger | open |

## Objects and players

| Text | Tree | Status |
|---|---|---|
| "any creature" | `Quant::One`, the same as "a creature" | open |
| "a second target" | `AnotherTarget` | open |
| "up to one target player or planeswalker" | an object whose kind is Player, not a separate player alternative | open |
| "each with power …" | the condition holds for every one; "each" is dropped | open |
| "with lesser mana value" | compared against the object the ability triggered on, which the text leaves unstated | open |

## Zones

| Text | Tree | Status |
|---|---|---|
| "put … on top" or "put them back" with no library named | `OwnersLibraryTop` | open |
| a bare "the top card" or "on the bottom" | the owner's library | open |
| "is put into exile" with no "from" | `from: Zone::Anywhere` | open |

## Effects

| Text | Tree | Status |
|---|---|---|
| "protection from each color" | protection from each of the five colors | open |
| "choose both" | `Choose::N(2)` | open |
| "one or more" | `AtLeast(N(1))` | open |
| "unless <condition>" | `If { Not(condition) }` | open |
| "… instead" after an effect, even on the previous line | `Effect::Instead` replaces the effect before it | open |
| "Repeat this process until …" | `RepeatUntil`, which repeats everything before it in the ability; nothing in the tree says how far back | open: the tree should say |
| ", whichever comes first" | consumed with no node, because "until A or B" already ends at the first | open |
| "Do this only once each turn" | a limit on the whole ability | open |
| "if it had counters" | the same as "has" | open |

## Durations

| Text | Tree | Status |
|---|---|---|
| a duration inside a `During` block | defaults to `Permanent` | open |

## Lexing

| Text | Tree | Status |
|---|---|---|
| any parenthesised text | dropped as reminder text, which loses rules text such as Mistform Ultimus's "(even if this card isn't on the battlefield)" | open: should keep parentheticals that are not reminder text |
