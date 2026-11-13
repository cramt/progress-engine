# Stack and priority

### 1. APNAP puts the last player's triggers on top, and reversing the turn order reverses who that is

**Cards:** [Wrath of God], [Blood Artist], [Mirage Mirror], [Aeon Engine]
**Board:** Three players, turn order P1, P2, P3; P1 active in the
precombat main phase. P2 is at 10 life and controls Blood Artist and an
untapped Aeon Engine. P3 is at 20 life and controls Mirage Mirror with {2}
available. Among them all, 25 creatures are on the battlefield, Blood
Artist included.
**Does:** P1 casts Wrath of God. P3 activates Mirage Mirror, copying Blood
Artist (26 creatures). P2 then activates Aeon Engine; it resolves. Mirage
Mirror's ability resolves, then Wrath of God.
**Then:** Each Blood Artist (P2's and P3's Mirror) triggers 26 times. With
the turn order reversed (P1, P3, P2), P3's triggers go on the stack before
P2's, so P2's resolve first: aimed at P3, the 20th drains P3 to 0, P3
loses as state-based actions are next checked, and P3's remaining triggers
cease to exist. P2 ends at 30. Control: without Aeon Engine, P2's
triggers go on first and P3's resolve first; P2 loses at the 10th.
**Because:** CR 603.3b, CR 101.4: triggered abilities go on the stack in
APNAP order, the active player first, then the others in turn order, so
the last one's resolve first. Aeon Engine's ruling: the reversed order is
what APNAP uses. CR 704.3, CR 800.4a: a player at 0 life loses as the
game checks state-based actions between resolutions, and the objects they
controlled on the stack cease to exist.
**Source:** [APNAP #mtg](https://www.youtube.com/watch?v=L0Z9BeNygsw)
**Engine:** Blood Artist's dies trigger and APNAP for triggers run;
missing are Mirage Mirror's "becomes a copy until end of turn" (README: a
copy effect that ends is not yet), reversing the turn order, and an
activated ability exiling its own source as a cost.

### 2. Day's Undoing on its own turn exiles itself

**Cards:** [Day's Undoing], [Mystic Sanctuary]
**Board:** Two players. P1's precombat main phase; Mystic Sanctuary in
P1's hand; P1 controls three Islands.
**Does:** P1 casts Day's Undoing; it resolves.
**Then:** Each player shuffles hand and graveyard into their library and
draws seven. The turn ends: Day's Undoing is in exile, not in P1's
graveyard, so on P2's turn and P1's next, Mystic Sanctuary entering
untapped can't target it.
**Because:** CR 724.1b: ending the turn exiles every object on the stack,
including the resolving spell, before it would be put into the graveyard
(CR 608.2n's last step never comes). CR 724.1d: the game skips to the
cleanup step.
**Source:** [My Cat Went to the ER Today. Here’s An Old Video #mtg](https://www.youtube.com/watch?v=d5Fe751Peow)
**Engine:** "end the turn" (CR 724.1), each player shuffling a hand and
graveyard into their library, and Mystic Sanctuary's "enters untapped"
trigger.

### 3. Choosing isn't targeting, and once a resolution has chosen there is no response

**Cards:** [Druid of Purification], [Tamiyo's Safekeeping]
**Board:** Three players, P1 active. P2 controls two enchantments, A and
B. P1 has Druid of Purification in hand. P2 has Tamiyo's Safekeeping in
hand and {G}.
**Does:** P1 casts Druid of Purification; it enters and its ability
triggers. P2 casts Tamiyo's Safekeeping on A in response. The trigger
resolves: P1 chooses A, P2 chooses nothing, P3 chooses B.
**Then:** A had hexproof and could still be chosen; it isn't destroyed
because it's indestructible. B is destroyed. P2 had no chance to act
between the choices and the destruction.
**Because:** CR 115.10a: what an ability chooses without the word
"target" isn't a target, so hexproof (CR 702.11b) doesn't stop it.
CR 608.2c, CR 117.3b: an ability's instructions are followed during its
resolution with no player getting priority until it's done. Druid of
Purification's ruling: choices are made in turn order and the chosen
permanents destroyed at once.
**Source:** [TR&CI 228: How Does Proliferate Actually Work - Can You Respond & Can You Choose Which Counters](https://www.youtube.com/watch?v=BDfJwNLtwQs)
**Engine:** hexproof and indestructible run; missing are "each player may
choose" a permanent and destroying what was chosen.
