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
| "When you do, if you control a red permanent, he deals …" | `WhenYouDoIf { cond, then }`, not `WhenYouDo([If { … }])` | settled: CR 603.12 makes it a triggered ability and CR 603.4 makes the "if" right after its condition an intervening if, checked as it triggers and again as it resolves; Ajani's ruling, "if you control a red permanent other than Ajani, a second reflexive ability triggers". Ajani, Earthbender Ascension, Sigil of Myrkul |
| "When a creature card is exiled this way, …" | `WhenThisWay`, a reflexive trigger | settled: CR 603.12 |
| "When that creature dies this turn, …", "Whenever you attack this turn, …" | `During { ThisTurn, [DelayedWhen … ] }` | settled: the delayed trigger lasts the turn |
| "If you do, at the beginning of your first upkeep, look at the top four cards …. You may put one of those cards back …. Exile the rest." | every sentence after the delayed trigger is its | settled: "those cards" are the ones it looked at, which exist only then |
| "Whenever you attack this turn, create two … tokens …. Sacrifice them at the beginning of the next end step." | the same for every delayed trigger said first in its sentence (`Delayed`, `DelayedWhen`, `DelayedWhenever`, alone or for the turn): the sentences after it are its | settled: CR 603.7; each attack makes its own tokens, so each sacrifices its own. Dalkovan Encampment, Phelia, the Pacts |
| "When that creature dies this turn, you gain 3 life. Target creature you control fights that creature." | the sentence after it is the spell's when it names a new target, or is a trigger of its own ("When …", "At …") | settled: a spell's targets are chosen as it is cast (CR 601.2c), so Time to Feed's fight happens as it resolves; Phantasmal Mount's two "When …" sentences are two delayed triggers |
| "Whenever you cast … during your turn", "… while this creature has a -1/-1 counter on it" | `Event::While` on the event | settled |
| "Whenever this creature attacks while saddled" | `While { Attacks(x), Is { x, State::Saddled } }` | settled: saddled is a designation of a permanent until end of turn (CR 702.171b), and the bare "while saddled" is said of the attacker |
| "Whenever a creature you control attacks alone" | `AttacksAlone`, apart from `Attacks` | settled: CR 506.5, the only creature declared as an attacker |
| "Whenever you scry or surveil" | `Or([Performs { You, "scry" }, Performs { You, "surveil" }])` | settled: two keyword actions, the player said once |
| "Whenever this creature enters or transforms into ~" | `TransformsInto { this creature, SelfRef }` | settled: "~" is the back face's own name, the same object (CR 201.5c) |

## Ability kinds

| Text | Tree | Status |
|---|---|---|
| "This spell can't be countered." on an instant or sorcery | `Static`, as on a permanent, not a `Spell` effect | settled: CR 113.6g, it functions on the stack; CR 113.3a, an instant's text is a spell ability only when it is not a static. Also under a condition (Exquisite Firecraft's spell mastery) |
| "At the beginning of your next upkeep, pay …. If you don't, you lose the game." on an instant or sorcery | `Spell([Delayed { … }])`, not a printed `Triggered` | settled: CR 603.7a, a delayed trigger created as the spell resolves; its line stays its own ability (CR 113.2c). Triggers that work from other zones ("When you cycle this card", "Whenever … , you may return this card from your graveyard") stay `Triggered` |
| "Buyback—Pay 3 life, Discard a card at random.", "Replicate—Pay {E}{E}{E}.", "Kicker—Forage.", "Splice onto Arcane—An opponent gains 5 life." | `Keywords`, the cost in words | settled: CR 702, a keyword with its cost; they read as a labelled spell before. "Pay {E}{E}" is the cost `{E}{E}` (CR 107.14) |
| "Max speed — {T}: Add {C}{C}.", "Exhaust — {3}: …", "Power-up — {4}{W}: …", "Boast — …", "Forecast — …", "Solved — …" | `KeywordOn { keyword, ability }`, not `Labelled` | settled: each is a keyword that adds rules to the ability after its dash (CR 702.178a, 702.177a, 702.193a, 702.142a, 702.57, 702.169), where an ability word adds none (CR 207.2c); they read as ability words before (143 corpus trees moved, none pinned). Exhaust, power-up, boast and forecast take only an activated ability. "Partner—Friends forever" and "Modular—Sunburst" are keywords too and still read as labels: open |
| "if this creature isn't prepared" | `Not(Is { State::Prepared })` | settled: a creature with a prepare spell is prepared or not (Codie's rulings). "Prepared" is read only there and after "becomes", not as an adjective, so "a prepared spell" (Codie) does not parse: what it names is not settled |
| "Start your engines!" | `Keywords`, the "!" part of the name | settled: CR 702.179a, a static keyword ability; Scryfall's catalog files it among the ability words |
| "I — Crescent Fang — Search …" | `Chapter { [Labelled { "crescent fang", … }] }` | settled: CR 207.2d, a flavour word has no rules meaning. Tried after the plain chapter, so "I, II — Choose one — •…" stays a modal chapter |

## Objects and players

| Text | Tree | Status |
|---|---|---|
| "any creature" | `Quant::One`, the same as "a creature" | settled: one, of the chooser's pick, either way |
| "a second target" | `SecondTarget`, not `AnotherTarget` | settled: CR 115.3, a second "target" may choose the first one again; "another" may not |
| "the other" (of two) | `Quant::Rest`, what the earlier choice left, not "it" | settled |
| "this creature or another creature" | `SelfOrAnother` keeps "this creature", as `SelfRef` does elsewhere | settled: the same object as "~", kept because the type line is not in the tree |
| "that player", "they", "the player" | `Player::That` | settled: one back-reference |
| "For each permanent destroyed this way, its controller …. Those players put the exiled cards onto the battlefield, then shuffle." | `Player::Those(ControllerOf(permanents destroyed this way))`, after the loop | settled: Indomitable Creativity's rulings, "All of the exiled cards are put onto the battlefield at the same time", and a player whose library had no artifact or creature exiles nothing and still shuffles, so they are the controllers, not the players who exiled a card. Winds of Abandon too |
| "You may put a creature card from your hand onto the battlefield. That creature gains haste. Sacrifice that creature at the beginning of the next end step." | the sentences inside the `May` | settled: the creature exists only if the option was taken. The rule: after a `may` or `if you do` that moved, made, found, chose or revealed a new object, a sentence first said of a bare "it", "them" or "that creature" is the branch's, and so is an "Otherwise" after an absorbed "if". "Put the rest …" is not. Through the Breach, Suppression Ray, Turntimber Symbiosis, Karn's −2 |
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
| "a creature card or a planeswalker card from your graveyard" | the zone on each card's filter | settled: a zone after a list of cards holds of each. Not after other kinds: "target spell, nonland permanent, or card in a graveyard" keeps it on the card. "From among the cards milled this way", "from among them" after a list of cards is a zone too (Airlift Chaplain, Frontier Seeker, Szarekh) |
| "target spell or nonland permanent an opponent controls", "target spell or permanent with mana value 1 or greater", "target spell or permanent that's red or green" | the qualifier on each kind's filter | settled: one article for the list, so what trails it qualifies the whole list, as long as it can hold of each kind: a spell has a controller (CR 601.2a) as a permanent does (CR 110.2), a card elsewhere has none (CR 108.4), and the zone-on-the-last-kind rule above is about zones only. Sink into Stupor, Press the Enemy, Divide by Zero, Aether Gust are the only four trees it spreads in, and each qualifier holds of a spell as of a permanent |
| "target artifact, enchantment, or creature with flying", "Walls and/or creatures with flying" | `with`/`without` a keyword on the last kind alone, which becomes its own filter in the `or` chain | settled: Atraxa's Fall lists "battle" before "creature with flying", pointless if flying held of the battle, and a Wall with flying is already a creature with flying (Elven Riders). A list of colors before one kind ("blue or black creature with flying", Wallop) is one kind, and the keyword holds of it. It was never row 68: the type list shared its posts since round 4 (13 corpus trees moved, none pinned) |
| "an instant card or a card with flash", "a Plains card or a creature card with mana value 3 or less" | the qualifier on the last kind only | settled: an article said again starts a noun phrase of its own |
| "creatures that are green and/or white", "that are tapped and attacking" | `ThatIs` with alternatives; the second is not read so | settled: the second is both states at once, which alternatives cannot say |
| "Gideon deals 1 damage" on Gideon the Oathless | `~` | settled: CR 201.5c, a card's shortened name is its name ("X the Y" as "X of Y" already was), and a planeswalker's first word when it is its own subtype |
| "tokens that are tapped and attacking" with nothing after | `Recipient::ControllerChooses` | settled: CR 508.4, its controller chooses what it attacks |
| "each with power …" | the condition holds for every one; "each" is dropped | settled: "each" restates what a filter says of every object |
| "with lesser mana value" | in a trigger's first effect, when the event names an object: `ManaValueOf(Triggering)`, the triggering object; after an earlier object (Kethek's sacrificed creature, Reinterpret's countered spell, Rushed Rebirth's "that creature"): that object, `Antecedent` | settled: Orah, Clement, Havi, Jackdaw Savior, Riveteers Ascendancy, Scrap Trawler and Kodama compare with the object that triggered them, and nothing else is named before |
| "Whenever a Mountain you control enters, if you control at least five other Mountains" | `Adj::OtherThan(Triggering)`, not `State::Other` | settled: Valakut's ruling, "five or more Mountains other than that new one". Only where the counted objects are the event's kind and the event's object is not the card itself (Valakut, Roiling Canopy); `State::Other` stays "other than this object" |
| "he deals damage", "put a +1/+1 counter on him", "he's a 3/4" | `SelfRef` | settled: CR 201.5c names a card by its name; Oracle text gives "he" or "she" only to a character card naming itself (every one of the 114 corpus lines does) |
| "Tyvar gains indestructible until end of turn. Tap it." | `SelfRef`: a bare "it" whose sentence before names only the card is the card | settled: English binds the pronoun to the nearest noun, and the effect before names nothing else (the cost's creature is in the cost). Not after a sentence with another object: "~ deals 2 damage to target creature. Tap it." taps the creature |
| "this enchantment deals 1 damage to them" | `Recipient::Player(That)` | settled: in all of Oracle text a bare "damage to them" is a player, the singular "they" (Roiling Vortex, Browbeat, Risk Factor, Razorkin Needlehead); objects are "each of them" |
| "the exiled card" in an ability that exiled nothing before it | `ExiledWith(SelfRef)`, the linked ability's card, not `Marked(Exiled)` | settled: CR 607.2a. On an instant or sorcery its paragraphs are one spell, so there "the exiled card" stays what another paragraph exiled |
| "cards exiled with it", "if a card is exiled with it" | `ExiledWith(SelfRef)` | settled: CR 607.2a, "exiled with [this object]"; all 19 corpus lines are the card itself |
| "from among the nonland permanents they control" | `Quant::All` | settled: who controls them says which they are; "the" points back to nothing |

## Zones

| Text | Tree | Status |
|---|---|---|
| "put … on top" or "put them back" with no library named | `OwnersLibraryTop` | settled: the library just named, which is the owner's (CR 400.3); "back" is the top they were looked at from |
| a bare "the top card" or "on the bottom" | the owner's library | settled: CR 400.3 |
| "Shuffle ~ into its owner's library" | `ShuffleInto { who: You, … }`: `who` follows the instruction, and the library is the owner's whatever the words | settled: CR 400.3, a card goes only to its owner's library, so the tree needs no zone; Green Sun's Zenith cast by a player who does not own it still goes to its owner's |
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
| "This artifact deals damage equal to …. If the revealed land card was a Mountain, this artifact deals double that damage instead." | `Instead { [ThatDamage { n: Twice(ThatDamage) }] }` | settled: the damage of the effect "instead" replaces, which is never dealt, so it is not `Amount::Damage` ("dealt this way"); the replaced effect is the event `ThatDamage` names (Goblin Charbelcher) |
| "any number of target creatures" | `AnyNumberOfTarget`, apart from "up to X target" | settled: no bound |
| "the next spell you cast this turn" | `WillCastThisTurn`, apart from "spells you've cast this turn" | settled |
| "any of that mana is spent on" | `SpentOf::Any`, apart from "you spend this mana to cast", `SpentOf::All` | settled: some of it, not all |
| "Add {R}{R}. If any of that mana is spent on a creature spell, it gains haste until end of turn." | `OnSpend { of, on, then }`, not `If { … }` | settled: CR 106.6, an additional effect on the spell the mana is spent on, applied as it is spent; an "if" checked as the ability resolves would always be false, since none of the mana is spent yet. Arena of Glory, Generator Servant, Opal Palace. "When that mana is spent to cast …" (Pyromancer's Goggles, Primal Amulet, Path of Ancestry) is a delayed trigger, `DelayedWhen`, as before |
| "cards revealed this way" | `Marked(Revealed)`, not "from among them" | settled |
| "Equip legendary creature {3}" | `CostFor`, the objects and the cost | settled |
| "choose artifact, creature, enchantment, instant, or sorcery" | `Choice::CardTypeAmong` | settled: a card type, not objects |
| "has base power and toughness X/X" | `Becomes` with that size | settled: CR 613.4b, the same layer as "becomes X/X" |
| "Each player shuffles …, then draws seven cards" | the draw continues "each player" | settled: a third-person verb continues the subject before it |
| "choose both", "choose two" | `Choose::N(2)`; the round trip says "both" only when the spell has two modes | settled: "both" is two of two, so the mode count decides the word |
| "one or more" | `AtLeast(N(1))` | open |
| "unless <condition>" | `If { Not(condition) }` | settled: "unless" is "if not" |
| "… instead" after an effect, even on the previous line | `Effect::Instead` replaces the effect before it | open |
| "Repeat this process until …", "draw a card and repeat this process" | `Process([…])` around the ability's instructions, with `RepeatUntil` or `RepeatProcess` inside it: again from the first instruction of the enclosing `Process` | settled: in all six corpus cards "this process" is the whole ability; Cultivator Colossus's ruling, "the process is repeated until you decline to put a land card onto the battlefield", repeats the land as well as the draw |
| ", whichever comes first" | consumed with no node, because "until A or B" already ends at the first | settled |
| "Do this only once each turn" | a limit on the whole ability | open |
| "if it had counters", "if it was a creature" | `Condition::Past`, as it was when the event happened | settled: last known information (CR 603.10a for leaving the battlefield), not as it is now |
| "protection from spells and from permanents that were cast this turn" | two `protection` keywords, "that were cast this turn" on the permanents alone | settled: CR 702.16g, two separate protection abilities; the post qualifies the noun it follows |
| "becomes a 2/2 creature with all creature types" | `Becomes` and `AlsoIs(EveryCreatureType)`, the same duration | settled: the second adds to the types the first sets |
| "… until end of turn. It's still a land." | `Keeps::Listed`, as "that's still a land" | settled: CR 205.1b names both forms |
| "he's a 3/4 Ninja creature" (Kaito) | `Is` (types replaced) and `BasePt` | settled: CR 205.1a, a type said without "still" or "in addition" replaces; a size with the types is a base size (CR 613.4b) |
| "target land becomes an Island", "Nonbasic lands are Mountains", "becomes a Coward" | `Keeps::AllButSubtypesOfSet` on `Becomes`, `TypeSet::Subtypes` on `Is`: only subtypes are set, so only their set is replaced | settled: CR 205.1a, new subtypes replace the subtypes "from the appropriate set", and CR 305.7, setting a land's subtype adds or removes no card type or supertype. Tide Shaper's land stays a land, Blood Moon's stays snow or legendary. `Keeps::Nothing` read literally replaced every type |
| "becomes an artifact creature", "becomes a 2/2 Construct artifact creature" | `Keeps::OtherTypes`, and `AllButSubtypesOfSet` with creature types | settled: CR 205.1b, "an 'artifact creature'" keeps all its prior card types and subtypes, a "[creature type] artifact creature" all but its creature types, which it replaces (26 corpus trees: the Chimeric artifacts, the Totems, Kenku Artificer). `BecomesSized` (Karn) now says it too. "In addition to its other types" said or not is a `syn` entry |
| "when each opponent casts their first spell of the game, counter that spell …" | `ForEach { opponents, [DelayedWhen { Casts { who: That, … } }] }` | settled: a delayed trigger triggers once (CR 603.7b), and Chancellor of the Annex's ruling has it trigger for each opponent, so it is one per opponent |
| "Create a … token. It has "…"" | the ability in the token's own list | settled: CR 111.3, the effect that creates a token defines what it has |
| "If you would draw one or more cards" | `DrawsOneOrMore`, apart from "draw a card" | settled: one instruction to draw several is several draws (CR 121.2); the first replaces the instruction, the second each draw |
| "The next time a source of your choice would deal damage to you this turn, prevent that damage" | `NextTime` with `PreventThatDamage` | settled: CR 609.7, 615.8, a one-shot shield on the next instance |
| "If that creature would die this turn, exile it instead" | `During { ThisTurn, [Replacement] }` | settled |
| "Choose target creature … You may pay any amount of {E}. ~ deals that much damage" | `PayEnergy(AnyAmount)`, then `ThatMany` | settled |
| "this is the first time this ability has resolved this turn … If it's the second time" | `NthResolution(n)` for both | settled: the second is the first said again without its tail |
| "Kicker {G} and/or {1}{U}" | `EitherOrBoth` | settled: CR 702.33d, two kicker costs, each may be paid |
| "return it to the battlefield with a -1/-1 counter on it" | `EntersWith` around the move, not a `PutCounters` after it | settled: CR 122.6, it is given the counters as it enters |
| "Counter target spell unless its controller pays {2}. If they do, you create a Lander token." | `IfYouDo` after the `Counter`, whose "do" is the paying | settled: the option before it was the payer's, so "they" is that player; the tree keeps the order, and the round trip says the payer |
| "~ deals 4 damage instead if …" | `Recipient::Same`: the recipient of the damage it replaces | settled: the text names none, and the replaced effect named one |
| "Destroy that creature if A instead if B" | `Instead { B, [If { A, destroy }] }` | open: as "… instead" above |
| "you may have this land deal 3 damage" | `May` around the land's own `Damage` | settled: after "may", an object's verb is said with "have"; who deals the damage is the land |
| "until the end of your next turn" | `UntilEndOfYourNextTurn`, apart from "until your next turn" | settled: the first lasts through that turn, the second ends as it begins |
| "if a nonland permanent left the battlefield this turn or a spell was warped this turn" | `Or([DidThisTurn(Leaves), DidThisTurn(CastUsing { "warp" })])` | settled: "a spell was warped" is one cast for its warp cost (CR 702.185c); "or" between two whole conditions is either one |
| "if this spell was cast using teamwork" | `CastUsing { SelfRef, "teamwork" }` | settled: CR 702.194b, whether its caster chose to pay that cost |
| "if at least four mana was spent to cast it", "if five or more mana was spent to cast that spell" | `AmountIs { ManaSpentToCast(it), Ge, 4 }` | settled: the total of any kind, as "the amount of mana spent to cast" |
| "if you've cast another spell this turn", "if you didn't cast a spell this turn" | `DidThisTurn(Casts { You, Another spell })`, `Not(…)` | settled: "another" is other than this spell; "didn't cast" and "haven't cast" with "this turn" are one test |
| "if two or more nonland permanents entered the battlefield under your control this turn", "if this land entered this turn" | `DidThisTurn(Enters { … })`, the count the quantity's | settled: the same event as "you had a land enter the battlefield under your control this turn" |
| "Activate only if this land entered this turn or if you control a basic land" | `Or([…, …])` | settled: "if" said again before the second changes nothing |
| "if there's a Lesson card in your graveyard" | `ThereAre([a Lesson card in your graveyard])` | settled: one object, as "there is an instant card and a sorcery card" is two |
| "as long as you have an enduring story" | `EnduringStory(You)` | settled: CR 702.195b, a designation storied gives |
| "As an additional cost to cast this spell, behold a Goblin and exile it", "… behold a Merfolk or pay {2}" | `Cost::ActionOn { "behold", a Goblin }`, then `Exile(it)`; `OneOf` for "or" | settled: CR 701.4a, reveal one from hand or choose one you control; "it" is the beheld object |
| "If a Dragon was beheld" | `Beheld(a Dragon)` | settled: CR 701.4b, the quality as it was beheld |
| "{1}{U}, Remove a counter from this creature:", "remove up to three counters from target creature" | `Cost::RemoveAnyCounters`, `Effect::RemoveAnyCounters`, apart from a named kind | settled: any kinds, as the payer or player picks |
| "When this creature dies, put its counters on target creature you control" | `PutThoseCounters { All counters on it, … }` | settled: every counter it had, kind by kind; "it" is the dying creature, as elsewhere in a trigger's effect |
| "If its mana value was 2 or less", "if its power was 3 or greater" | `Past(AmountIs { … })` | settled: as it was when the event happened, as "if it was a creature" is (CR 603.10a for one that left) |
| "Two target creatures each get +2/+2", "They each get +2/+2" | `Modify`, the "each" dropped | settled: "each" after a plural subject restates what a bonus gives each of them, as "each of" before it does |
| "This creature must be blocked if able" | `Restrict { MustBeBlocked }` | settled: a requirement on the declaration of blockers, obeyed as far as the restrictions allow (CR 509.1c, which names the kind "must block") |
| "Spend this mana only to activate abilities" | `SpendUse::ActivateAny` | settled: abilities of any object, apart from "abilities of creatures" |
| "your speed" | `Amount::Speed(You)`, 0 for a player with none | settled: CR 702.179f |
## Durations

| Text | Tree | Status |
|---|---|---|
| a duration inside a `During` block | defaults to `Permanent` | open |

## Lexing

| Text | Tree | Status |
|---|---|---|
| any parenthesised text | dropped as reminder text, which loses rules text such as Mistform Ultimus's "(even if this card isn't on the battlefield)" | open: should keep parentheticals that are not reminder text |
