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
| "When you do, put … on target attacking creature. It becomes an Angel …" | the "It …" sentence inside `WhenYouDo` | settled: "it" is what the reflexive trigger targeted, which exists only when that trigger resolves |
| "When you do, if you control a red permanent, he deals …" | `WhenYouDo([If { … }])`, as earlier trees read it | open: the form is CR 603.4's intervening-if, which would check as it triggers too |
| "When a creature card is exiled this way, …" | `WhenThisWay`, a reflexive trigger | settled: CR 603.12 |
| "When that creature dies this turn, …", "Whenever you attack this turn, …" | `During { ThisTurn, [DelayedWhen … ] }` | settled: the delayed trigger lasts the turn |
| "If you do, at the beginning of your first upkeep, look at the top four cards …. You may put one of those cards back …. Exile the rest." | every sentence after the delayed trigger is its | settled: "those cards" are the ones it looked at, which exist only then |
| "Whenever you attack this turn, create two … tokens …. Sacrifice them at the beginning of the next end step." | the same for every delayed trigger said first in its sentence (`Delayed`, `DelayedWhen`, `DelayedWhenever`, alone or for the turn): the sentences after it are its | settled: CR 603.7; each attack makes its own tokens, so each sacrifices its own. Dalkovan Encampment, Phelia, the Pacts |
| "When that creature dies this turn, you gain 3 life. Target creature you control fights that creature." | the sentence after it is the spell's when it names a new target, or is a trigger of its own ("When …", "At …") | settled: a spell's targets are chosen as it is cast (CR 601.2c), so Time to Feed's fight happens as it resolves; Phantasmal Mount's two "When …" sentences are two delayed triggers |
| "Whenever you cast … during your turn", "… while this creature has a -1/-1 counter on it" | `Event::While` on the event | settled |

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
| "up to one other target creature or spell" | `Quant::OtherTarget`: "other" holds of every kind the filter lists | settled: said once before "target", it qualifies the whole noun phrase |
| "players can't …", "each player can't …" | `Player::Each` | settled: a rule said of every player, one at a time or together |
| "its graveyard", "its library", "its hand" | `OwnerOf(it)`, the same tree as "its owner's graveyard" | settled: CR 400.3, an object's hand, library or graveyard is only ever its owner's |
| "its controller's graveyard" | `ControllerOf(it)`, apart from "its graveyard" | settled: the player controlling it, who for a stolen permanent or a spell another player controls is not its owner |
| "a creature with the greatest power" | `Quant::One`, the same as "the creature with the greatest power" | settled: one object with that value; where several tie, the chooser picks |
| "a graveyard has twenty or more cards in it" | `ZoneHasCards { AGraveyard, … }`: some one graveyard | settled: "a" is any one, not all graveyards counted together |
| "it's the second creature spell you cast this turn" | `NthSpellThisTurn`, counting this spell among them | settled |
| "cards you don't own", "cards your opponents own" | `NotOwnedBy(You)` and `Owner(EachOpponent)`, apart | settled: in Two-Headed Giant a teammate owns cards too (CR 810.1); earlier trees read the first as the second |
| "choose target opponent", "choose an opponent" | `Choice::TargetOpponent` and `Choice::Opponent`, apart | settled: only the first targets (CR 115), so only it can fizzle or be stopped by hexproof; earlier trees read both as the second |
| "a black or red permanent, spell, or card" | the colors on each kind's filter | settled: said before the list, they qualify every kind in it |
| "a creature card or a planeswalker card from your graveyard" | the zone on each card's filter | settled: a zone after a list of cards holds of each. Not after other kinds: "target spell, nonland permanent, or card in a graveyard" keeps it on the card |
| "creatures that are green and/or white", "that are tapped and attacking" | `ThatIs` with alternatives; the second is not read so | settled: the second is both states at once, which alternatives cannot say |
| "Gideon deals 1 damage" on Gideon the Oathless | `~` | settled: CR 201.5c, a card's shortened name is its name ("X the Y" as "X of Y" already was), and a planeswalker's first word when it is its own subtype |
| "tokens that are tapped and attacking" with nothing after | `Recipient::ControllerChooses` | settled: CR 508.4, its controller chooses what it attacks |
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
| "choose both", "choose two" | `Choose::N(2)`; the round trip says "both" only when the spell has two modes | settled: "both" is two of two, so the mode count decides the word |
| "one or more" | `AtLeast(N(1))` | open |
| "unless <condition>" | `If { Not(condition) }` | settled: "unless" is "if not" |
| "… instead" after an effect, even on the previous line | `Effect::Instead` replaces the effect before it | open |
| "Repeat this process until …" | `RepeatUntil`, which repeats everything before it in the ability; nothing in the tree says how far back | open: the tree should say |
| ", whichever comes first" | consumed with no node, because "until A or B" already ends at the first | settled |
| "Do this only once each turn" | a limit on the whole ability | open |
| "if it had counters", "if it was a creature" | `Condition::Past`, as it was when the event happened | settled: last known information (CR 603.10a for leaving the battlefield), not as it is now |
| "protection from spells and from permanents that were cast this turn" | two `protection` keywords, "that were cast this turn" on the permanents alone | settled: CR 702.16g, two separate protection abilities; the post qualifies the noun it follows |
| "becomes a 2/2 creature with all creature types" | `Becomes` and `AlsoIs(EveryCreatureType)`, the same duration | settled: the second adds to the types the first sets |
| "… until end of turn. It's still a land." | `Keeps::Listed`, as "that's still a land" | settled: CR 205.1b names both forms |
| "he's a 3/4 Ninja creature" (Kaito) | `Is` (types replaced) and `BasePt` | settled: CR 205.1a, a type said without "still" or "in addition" replaces; a size with the types is a base size (CR 613.4b) |
| "Create a … token. It has "…"" | the ability in the token's own list | settled: CR 111.3, the effect that creates a token defines what it has |
| "If you would draw one or more cards" | `DrawsOneOrMore`, apart from "draw a card" | settled: one instruction to draw several is several draws (CR 121.2); the first replaces the instruction, the second each draw |
| "The next time a source of your choice would deal damage to you this turn, prevent that damage" | `NextTime` with `PreventThatDamage` | settled: CR 609.7, 615.8, a one-shot shield on the next instance |
| "If that creature would die this turn, exile it instead" | `During { ThisTurn, [Replacement] }` | settled |
| "Choose target creature … You may pay any amount of {E}. ~ deals that much damage" | `PayEnergy(AnyAmount)`, then `ThatMany` | settled |
| "this is the first time this ability has resolved this turn … If it's the second time" | `NthResolution(n)` for both | settled: the second is the first said again without its tail |
| "Kicker {G} and/or {1}{U}" | `EitherOrBoth` | settled: CR 702.33d, two kicker costs, each may be paid || "return it to the battlefield with a -1/-1 counter on it" | `EntersWith` around the move, not a `PutCounters` after it | settled: CR 122.6, it is given the counters as it enters |
| "~ deals 4 damage instead if …" | `Recipient::Same`: the recipient of the damage it replaces | settled: the text names none, and the replaced effect named one |
| "Destroy that creature if A instead if B" | `Instead { B, [If { A, destroy }] }` | open: as "… instead" above |
| "you may have this land deal 3 damage" | `May` around the land's own `Damage` | settled: after "may", an object's verb is said with "have"; who deals the damage is the land |
| "until the end of your next turn" | `UntilEndOfYourNextTurn`, apart from "until your next turn" | settled: the first lasts through that turn, the second ends as it begins |
## Durations

| Text | Tree | Status |
|---|---|---|
| a duration inside a `During` block | defaults to `Permanent` | open |

## Lexing

| Text | Tree | Status |
|---|---|---|
| any parenthesised text | dropped as reminder text, which loses rules text such as Mistform Ultimus's "(even if this card isn't on the battlefield)" | open: should keep parentheticals that are not reminder text |
