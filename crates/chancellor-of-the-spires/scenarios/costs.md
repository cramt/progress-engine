# Costs

### 1. Increment counts the mana spent, not the mana value

**Cards:** [Pensive Professor], [Prismari Pianist], [Cyclonic Rift], [Deadly Rollick], [Treasure Cruise]
**Board:** P1 controls Prismari Pianist and two Pensive Professors: A with
six +1/+1 counters (6/8) and B with one (1/3). P1 controls a commander on
the battlefield, has nine lands untapped and seven cards in graveyard. P2
controls a creature. Each case below starts from this board.
**Does:** (a) P1 casts Cyclonic Rift for its overload cost, spending seven
mana. (b) P1 casts Deadly Rollick without paying its mana cost. (c) P1
casts Treasure Cruise, exiling seven cards from the graveyard with delve
and spending {U}.
**Then:** (a) Increment triggers on both Professors (7 is greater than 6
and than 1); each gets a counter and draws a card. Prismari Pianist makes
one Elemental, not three: Cyclonic Rift's mana value is 2. (b) No
increment trigger: no mana was spent; Pianist makes one Elemental. (c) No
increment trigger on either Professor (1 is not greater than 1 or 3, nor
than 6 or 8); Pianist makes three Elementals, since Treasure Cruise's
mana value is 8 however little was paid.
**Because:** CR 702.191a: increment compares "the amount of mana spent to
cast that spell" with power or toughness, an intervening "if" checked
again on resolution (CR 603.4, Pensive Professor's ruling). CR 118.9c:
an alternative cost doesn't change the mana value. CR 702.66a,
CR 702.66b: delve exiles cards instead of paying mana, so that mana isn't
spent.
**Source:** [TR&CI 220: Increment - How It Differs From Evolve, What Doesn't Work, & The Intervening "If" Clause](https://www.youtube.com/watch?v=EgtizSyI1NA)
**Engine:** increment, mana spent tracked per spell, overload, delve, and
"if you control a commander" free casting.

### 2. Power-up costs the permanent's mana cost less on the turn it entered, commander tax or not, and only once

**Cards:** [Thanos, the Mad Titan], [Stifle]
**Board:** Commander. Thanos, the Mad Titan is P1's commander and has been
cast from the command zone once before. P1's precombat main phase.
**Does:** P1 casts Thanos from the command zone for {2}{R}{W}{B}. In the
same turn P1 activates its power-up. P2 counters the ability with Stifle.
**Then:** The power-up cost {C}{U}{G}: {C}{W}{U}{B}{R}{G} less {R}{W}{B}.
After Stifle, P1 can't activate it again on that Thanos, though it never
resolved. If Thanos leaves and returns, the new object can activate it
again, reduced again if it entered that turn.
**Because:** CR 702.193a: the reduction is by the permanent's mana cost if
it entered this turn, and "Activate this ability only once". CR 903.8:
the tax is an additional cost to cast, not part of the mana cost.
CR 702.193b, CR 118.7: coloured symbols reduce their own colour.
CR 400.7: a permanent that leaves and returns is a new object.
**Source:** [TR&CI 224: Marvel Set Mechanics Rules - Power-Up & Teamwork (Command Tax, Fizzle, Flicker, & More)](https://www.youtube.com/watch?v=PMbSdRWz1S4), [Marvel Spoilers Day 1 - Discussing The 70+ New Cards From The Upcoming Marvel Set](https://www.youtube.com/watch?v=2zTqj8cPqNk)
**Engine:** power-up's reduction and once-only limit run (Quicksilver,
README, *The milestone*), and Stifle runs; missing are Thanos's "choose
odd or even" and the commander tax being kept apart from the power-up
reduction, worth a test.

### 3. Teamwork can tap summoning-sick creatures, and counts their power before tapping

**Cards:** [Earth's Mightiest Heroes], [Plargg, Dean of Chaos // Augusta, Dean of Order]
**Board:** P1 controls Augusta, Dean of Order (on the battlefield as its
back face) and a 4/4 creature that entered this turn; Augusta is tapped.
The 4/4 is untapped, so Augusta gives it +0/+1: it is a 4/5.
**Does:** P1 casts Earth's Mightiest Heroes and wants to pay teamwork 5
by tapping the 4/4.
**Then:** P1 can't: its power is 4 when it would be tapped, and the +1/+0
it would get once tapped doesn't count. With another untapped 1-power
creature also tapped, P1 can pay, summoning sickness notwithstanding,
and puts any number of the revealed creature cards onto the battlefield.
**Because:** CR 702.194a: teamwork taps creatures "with total power N or
more" as an additional cost, the power read as they are chosen to pay
(CR 601.2h). CR 302.6: summoning sickness restricts attacking and {T}
abilities of the creature itself, not tapping it for another cost.
**Source:** [TR&CI 224: Marvel Set Mechanics Rules - Power-Up & Teamwork (Command Tax, Fizzle, Flicker, & More)](https://www.youtube.com/watch?v=PMbSdRWz1S4)
**Engine:** teamwork and an optional additional cost tapping creatures
(Phyrexian Dreadnought's "sacrifice total power 12" is the nearest that
runs), and modal double-faced cards' back faces.

### 4. X is 0 when a spell is cast free, unless its own text defines X

**Cards:** [Omniscience], [Braingeyser], [Spoils of War]
**Board:** P1 controls Omniscience and has Braingeyser and Spoils of War in
hand. P2's graveyard holds four artifact or creature cards. P1 controls a
creature.
**Does:** P1 casts Braingeyser without paying its mana cost, then Spoils of
War the same way.
**Then:** Braingeyser's X must be 0: the target draws no cards. Spoils of
War's X is 4, set by its text as it is cast, and P1 distributes four +1/+1
counters among its targets.
**Because:** CR 107.3b: casting without paying the mana cost or an
alternative cost with X makes 0 the only legal X, where the spell's text
doesn't define it. CR 107.3c: where it does, that is X while the spell is
on the stack. Spoils of War's ruling: X is set as it is cast.
**Source:** [TR&CI 222: X Spells Are NOT Always Zero When Cast Via Omniscience & Other Free Alternative Costs](https://www.youtube.com/watch?v=wQvW3SBCmGI)
**Engine:** `{X}` is refused today (README), as is distributing counters.

### 5. Flashback granted by the card Flashback: the mana cost, X chosen, one alternative cost, additional costs still apply

**Cards:** [Flashback], [Earthquake], [Cyclonic Rift], [Deflecting Swat], [Big Score], [Rite of Replication], [Faithless Looting], [Ancestral Vision], [Mission Briefing]
**Board:** P1's graveyard holds each of Earthquake, Cyclonic Rift,
Deflecting Swat, Big Score, Rite of Replication, Faithless Looting and
Ancestral Vision. P1 controls a commander. Each case starts with P1
casting Flashback targeting one of them.
**Does / Then:**
- Earthquake gains flashback {X}{R}; P1 chooses X and pays it.
- Cyclonic Rift gains flashback {1}{U}, and P1 can't pay its overload
  cost: both are alternative costs. Cast instead from Mission Briefing's
  permission, it can be overloaded.
- Deflecting Swat gains flashback {2}{R}, and P1 can't also cast it
  without paying its mana cost.
- Big Score cast with flashback still needs a card discarded.
- Rite of Replication cast with flashback may be kicked for {5}.
- Faithless Looting has two flashback costs, {2}{R} and {R}; P1 chooses
  either.
- Ancestral Vision has no mana cost, so it gains no flashback cost and
  can't be cast this way.
- Any spell cast with flashback is exiled when it leaves the stack, even
  if countered or its targets are gone.
**Because:** CR 702.34a: flashback is an alternative cost, with the
exile. CR 118.9a: only one alternative cost per spell. CR 118.8,
CR 702.33a: additional costs, kicker among them, still apply. CR 202.1b,
CR 118.6: no mana cost is an unpayable cost. Flashback's rulings: X is
chosen and paid, and a card with several flashback costs may use any.
Mission Briefing gives permission, not an alternative cost.
**Source:** [TR&CI 217: Flashback Deep Dive - What Can & Can't You Do With It](https://www.youtube.com/watch?v=jBMR4NEZhtQ)
**Engine:** flashback from a graveyard and its exile run (Deep Analysis);
missing are flashback granted by another card, `{X}`, overload,
"without paying its mana cost" as an alternative, and Mission Briefing's
"you may cast it this turn".
