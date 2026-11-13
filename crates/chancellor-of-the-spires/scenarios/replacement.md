# Replacement and "can't" effects

### 1. Rain of Gore doesn't turn lifelink from combat damage into life loss

**Cards:** [Rain of Gore], [Lyra Dawnbringer]
**Board:** P2 controls Rain of Gore and two untapped 1/1 creatures, and is
at 30 life. P1 is at 20 life and controls Lyra Dawnbringer and three other
Angels, each a 4/4 without lifelink before Lyra's bonus. P1's turn.
**Does:** P1 attacks P2 with all four Angels. P2 blocks two of the
non-Lyra Angels with its 1/1s. Combat damage: Lyra in the first-strike
step, the other three in the regular step.
**Then:** P1 gains 5 life in the first-strike step and 15 in the regular
step, ending at 40. P2 loses 10 (Lyra and the unblocked 5/5) and is at 20,
and both blockers die. Rain of Gore applies to none of it.
**Because:** CR 702.15a, CR 702.15b: lifelink is a static ability and the
damage itself causes the life gain. CR 510.2: combat damage is dealt as a
turn-based action, not by a spell or ability, so "a spell or ability would
cause its controller to gain life" doesn't match. Rain of Gore's Oracle
ruling (2013-04-15) says the same. The second video explains this is
because lifelink stopped being a triggered ability (it became static in
Magic 2010).
**Source:** [Gore #mtg](https://www.youtube.com/watch?v=LiPijGQQkHA), [The History of Life Gain in #MTG](https://www.youtube.com/watch?v=GaoKA5VYwCA)
**Engine:** Lyra's "Other Angels you control get +1/+1 and have
lifelink", first strike and lifelink run; Rain of Gore's replacement is
carried, not run.

### 2. Rain of Gore does apply when a spell makes a lifelink creature deal damage

**Cards:** [Rain of Gore], [Lyra Dawnbringer], [Zealous Conscripts], [Fall of the Hammer]
**Board:** As scenario 1 after combat, with P2 at 20 and P1 at 40; P2's
turn. P2 controls Rain of Gore.
**Does:** P2 casts Zealous Conscripts, gaining control of Lyra Dawnbringer.
Then P2 casts Fall of the Hammer: Lyra deals damage to one of P1's Angels.
**Then:** While P2 controls Lyra, P1's other Angels are 4/4s without
lifelink. Lyra deals 5 damage and the Angel dies. P2, Lyra's controller,
would gain 5 life; Rain of Gore replaces that and P2 loses 5, ending at
15.
**Because:** CR 702.15b: lifelink gives life to the source's controller.
CR 614.1a: Rain of Gore replaces the gain when a spell (Fall of the Hammer,
controlled by P2) would cause its controller to gain life; Rain of Gore's
ruling excludes only combat damage. CR 613.1b: control changes in layer 2,
so Lyra's bonus now applies to P2's Angels.
**Unsure:** the CR doesn't define when a spell "would cause" lifelink's
gain; the answer rests on the reading the ruling implies by excluding only
combat damage.
**Source:** [Gore #mtg](https://www.youtube.com/watch?v=LiPijGQQkHA)
**Engine:** control-changing effects (layer 2, not yet), "untap that
permanent" and haste until end of turn, a creature dealing damage
"equal to its power" to another target, and the replacement itself.

### 3. A player who can't gain life can't exchange up, so the exchange doesn't happen at all

**Cards:** [Command the Dreadhorde], [Sunspine Lynx], [Mister Negative]
**Board:** P1 is at 12 life and controls exactly one nonbasic land. P2 is
at 25 life and controls no nonbasic lands. Sunspine Lynx is in P2's
graveyard, Mister Negative in P1's. P1's precombat main phase.
**Does:** P1 casts Command the Dreadhorde targeting both cards. It
resolves: P1 is dealt 11 damage (4 + 7) and goes to 1; both enter under
P1's control. P1 puts Sunspine Lynx's trigger on the stack first and
Mister Negative's on top, targeting P2, and chooses to exchange.
**Then:** The exchange doesn't happen: P1 stays at 1 and P2 at 25, and P1
draws no cards. Then Sunspine Lynx deals 1 damage to P1, who loses the
game.
**Because:** CR 119.7, CR 701.12c: a player who can't gain life can't
be given a higher life total by an exchange, and CR 701.12a: if the whole
exchange can't be completed, none of it happens. Sunspine Lynx's "Players
can't gain life" applies as soon as it is on the battlefield.
**Source:** [Swapping Lives #mtg](https://www.youtube.com/watch?v=sw8fjV8nxvk)
**Engine:** a "players can't gain life" rule, exchanging life totals, and
Command the Dreadhorde's damage "equal to the total mana value of those
cards" (`Effect::Move` from a graveyard is refused today).

### 4. Two "double all damage" effects on one creature quadruple it

**Cards:** [Mjölnir, Hammer of Thor], [Wolverine, Best There Is]
**Board:** P1 controls Wolverine, Best There Is (2/2) equipped with
Mjölnir, Hammer of Thor. P2 is at 20 life. P1's turn, Wolverine untapped
and not summoning sick.
**Does:** P1 attacks P2 with Wolverine; P2 doesn't block.
**Then:** Wolverine deals 8 damage; P2 is at 12.
**Because:** Both are replacement effects (CR 614.1a), each applying once
to the damage event (CR 614.5), in the order P2 chooses as the affected
player (CR 616.1); 2 doubled twice is 8 either way. Wolverine is a red and
green legendary non-Villain, so Mjölnir's equip worthy {1} can attach to
it.
**Source:** [Marvel Spoilers Day 1 - Discussing The 70+ New Cards From The Upcoming Marvel Set](https://www.youtube.com/watch?v=2zTqj8cPqNk)
**Engine:** damage replacement effects and "equip worthy".

### 5. Library of Leng and Lorehold's "you may discard a card. If you do, draw"

**Cards:** [Lorehold, the Historian], [Library of Leng], [Big Score]
**Board:** P1 controls Lorehold, the Historian and Library of Leng, with
cards in hand. P2's upkeep begins.
**Does:** Lorehold's trigger resolves; P1 discards a card.
**Then:** The video's answer: P1 may put the discarded card on top of the
library instead of the graveyard, then draws it. Control, settled: a card
discarded as Big Score's additional cost to cast it can't be put on top
with Library of Leng.
**Because:** CR 118.12: "you may discard a card. If you do, draw" makes
the discard a cost paid as the ability resolves. Library of Leng's ruling
(2004-10-04) says it doesn't apply to a discard as a cost "because costs
aren't effects"; CR 609.1 says an effect is what happens as a result of a
spell or ability resolving. CR 118.8 for Big Score.
**Unsure:** whether a cost paid while a triggered ability resolves is "an
effect causing you to discard". The video argues yes from CR 609.1 against
Library of Leng's ruling, which predates CR 118.12's wording; neither the
CR nor a ruling settles it, so the engine should not assert either until
one does.
**Source:** [TR&CI 221: Lorehold & Leng - It Shouldn't Work, But It Does, But It Shouldn't... But It Does](https://www.youtube.com/watch?v=L0Kkt4aK5zE)
**Engine:** "you may discard. If you do" as a cost on resolution, an
upkeep trigger on each opponent's upkeep (upkeep triggers run), and a
replacement on discard.
