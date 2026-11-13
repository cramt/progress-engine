# Triggers

### 1. A "from anywhere" graveyard trigger destroyed in the same event doesn't trigger, while a dies trigger sees itself

**Cards:** [Culling Ritual], [Blood Artist], [Bloodchief Ascension], [Wrath of God]
**Board:** Two players at 20 life. P1's precombat main phase. P1 controls
Blood Artist and two other nontoken creatures with mana value 2 or less,
and no other nonland permanents. P2 controls Bloodchief Ascension with
three quest counters on it and no other nonland permanents.
**Does:** P1 casts Culling Ritual. Both players pass; it resolves.
**Then:** Blood Artist, P1's two creatures and Bloodchief Ascension are
destroyed at once, and P1 adds four mana, each {B} or {G}. Blood Artist
triggers three times (for itself and the two others); with P2 as each
target, P2 ends at 17 and P1 at 23. Bloodchief Ascension triggers zero
times: not for the three creature cards put into P1's graveyard, nor for
Culling Ritual put there after. Control: if P1 casts Wrath of God instead,
Bloodchief Ascension survives and triggers four times: once for each of
the three creature cards put into P1's graveyard, and once for Wrath of
God as it finishes resolving.
**Because:** CR 603.10, CR 603.10a: only leaves-the-battlefield abilities
and the few others listed look back in time. CR 603.6c: an ability that
triggers on a card put into a zone "from anywhere" is never a
leaves-the-battlefield ability, so Bloodchief Ascension, gone after the
event, doesn't see it. Blood Artist's "dies" is a leaves-the-battlefield
ability and looks back at itself.
**Source:** [Don’t Look Back #mtg](https://www.youtube.com/watch?v=Kob3_FXMASw)
**Engine:** Blood Artist's dies trigger looking back is covered (README,
*The game*); missing are a trigger on a card put into a graveyard "from
anywhere", an intervening "if" counting counters (CR 603.4 beyond "if you
control"), and Culling Ritual's mana "for each permanent destroyed this
way".

### 2. A creature whose "from anywhere" trigger watches its own graveyard doesn't trigger for itself as it dies

**Cards:** [Sefris of the Hidden Ways], [Wrath of God]
**Board:** P1 controls Sefris of the Hidden Ways and two other creatures,
has no dungeon in the command zone and has not ventured this turn.
**Does:** Wrath of God resolves.
**Then:** Sefris and both creatures are put into P1's graveyard and
Sefris's first ability doesn't trigger: P1 has no dungeon and hasn't
ventured.
**Because:** CR 603.6c, CR 603.10a: "put into your graveyard from
anywhere" is never a leaves-the-battlefield ability, so it doesn't look
back, and Sefris no longer exists on the battlefield after the event. The
same rule as scenario 1, seen from the source's own side.
**Source:** [I got the rules WRONG #mtg @JudgingFtW](https://www.youtube.com/watch?v=g9RJ26lwvcg)
**Engine:** a "one or more creature cards put into your graveyard from
anywhere" trigger, "triggers only once each turn", and venture into the
dungeon.

### 3. Wan Shi Tong shuffled into its library by Sway of the Stars still triggers

**Cards:** [Wan Shi Tong, All-Knowing], [Sway of the Stars]
**Board:** Two or more players. P1's precombat main phase. P1 controls Wan
Shi Tong, All-Knowing. Every player has cards in hand and graveyard and
permanents on the battlefield.
**Does:** P1 casts Sway of the Stars. It resolves.
**Then:** Every player's hand, graveyard and permanents, Wan Shi Tong
among them, are shuffled into their libraries; each player draws seven
cards and is at 7 life. Wan Shi Tong's library ability has triggered
once; P1 controls it, it goes on the stack after Sway of the Stars
finishes, and as it resolves P1 creates two 1/1 colorless Spirit tokens
with "This token can't block or be blocked by non-Spirit creatures."
**Because:** CR 603.10a: an ability that triggers when an object all
players can see is put into a hand or library looks back in time, so Wan
Shi Tong triggers though it left the battlefield in that event. "One or
more cards" makes the shuffle one trigger (CR 603.2c), and P1 controlled
the source when it triggered (CR 603.3a).
**Source:** [I got the rules WRONG #mtg @JudgingFtW](https://www.youtube.com/watch?v=g9RJ26lwvcg)
**Engine:** a trigger on cards put into a library, looking back (README:
"triggers on any other event" is not yet), and Sway of the Stars' "each
player" shuffle and life total becoming 7.

### 4. Triggers during a resolution belong to whoever controlled their source then, and choose targets only as they go on the stack

**Cards:** [Dack Fayden, Helping Hand], [Venser, Fervent Forger], [Archon of Cruelty], [Avacyn, Angel of Horror], [Jace, Multiverse Architect]
**Board:** Four-player Commander, P1 active in the precombat main phase.
P1's library is stacked so that Dack Fayden's reveal finds Venser, Fervent
Forger, then Archon of Cruelty, then Avacyn, Angel of Horror.
**Does:** Dack Fayden, Helping Hand enters under P1's control (the video
puts it there with Jace, Multiverse Architect's −3, exiling a token P1
controls; casting Dack is the same test). Its enters ability resolves: the
three creatures enter under P1's control, goaded, and P1 gives Venser to
P2, Archon to P3 and Avacyn to P4.
**Then:** Venser's and Archon's enters abilities triggered as they
entered, under P1's control, and are put on the stack only after Dack's
ability has finished. Both are P1's. Venser's second mode may target
Venser itself, which P2 now controls. If it does, P1 creates two token
copies of Venser with haste and keeps one under the legend rule, and both
tokens' enters abilities have triggered under P1's control, so P1 can aim
one at P2's Venser again and the other at P3's Archon. Archon's
ability targets an opponent of P1 (that player sacrifices, discards, loses
3 life; P1 draws and gains 3).
**Because:** CR 603.3: a triggered ability is put on the stack the next
time a player would receive priority, never mid-resolution. CR 603.3a: its
controller is whoever controlled its source when it triggered. CR 115.1d,
CR 603.3d: its targets are chosen as it is put on the stack, so "an
opponent controls" is read then. CR 603.6a, CR 704.5j: both tokens
entered and triggered before the legend rule removed one.
**Source:** [Precons these days #mtg](https://www.youtube.com/watch?v=TsYV0FfBWEc)
**Engine:** control-changing effects (layer 2, not yet), Dack's "reveal
until X creature cards" and "choose a different opponent", Venser's modal
enters ability (`Effect::Modal` is refused today, Boros Charm in the
worklist), goad, and Jace as a planeswalker (refused).

### 5. Mister Fantastic draws for a Tivit council's dilemma once for the Treasures and once for each investigate

**Cards:** [Tivit, Seller of Secrets], [Mister Fantastic, Reed Richards]
**Board:** Four-player game. P1 controls Tivit, Seller of Secrets and
Mister Fantastic, Reed Richards. Tivit is attacking P2, unblocked.
**Does:** Tivit deals combat damage to P2; its council's dilemma
resolves. P1 votes twice, both bribery (Tivit's additional vote); P2 votes
bribery; P3 and P4 vote evidence.
**Then:** Tivit's ability finishes before anything goes on the stack: P1
has three Treasure tokens and two Clue tokens. Then Mister Fantastic's
abilities go on the stack: three of them, as both videos say, if the
Treasures enter as one event and each investigate is its own; P1 may draw
three cards.
**Because:** CR 603.3, CR 701.16a ("investigate" means create a Clue
token). Mister Fantastic triggers once per event in which one or more
tokens enter (CR 603.2c).
**Unsure:** the count rests on "For each evidence vote, investigate" being
two separate token-creation events and "For each bribery vote, create a
Treasure token" being one. CR 701.16a defines investigate but says nothing
about repetition, and no ruling on Tivit, Mister Fantastic or Erdwal
Illuminator settles it.
**Source:** [Reed and his Council’s Dilemma #mtg](https://www.youtube.com/watch?v=SLCis7PTW5s), [Marvel Spoilers Day 1 - Discussing The 70+ New Cards From The Upcoming Marvel Set](https://www.youtube.com/watch?v=2zTqj8cPqNk)
**Engine:** voting (council's dilemma), Treasure and Clue as predefined
tokens (README: "a predefined token" is not yet), and a trigger on tokens
entering.

### 6. Creating a Clue without the word investigate isn't investigating

**Cards:** [Forecasting Fortune Teller], [Erdwal Illuminator]
**Board:** P1 controls Erdwal Illuminator and has not investigated this
turn.
**Does:** P1 casts Forecasting Fortune Teller; its enters ability
resolves.
**Then:** P1 has one Clue token and Erdwal Illuminator has not triggered.
Control: if P1 then investigates once that turn, Erdwal Illuminator
triggers and P1 ends with three Clues.
**Because:** CR 701.16a: to investigate is to create a Clue, but an
ability that triggers when you investigate triggers on that keyword
action, and Forecasting Fortune Teller's ability creates a Clue without
investigating.
**Source:** [Reed and his Council’s Dilemma #mtg](https://www.youtube.com/watch?v=SLCis7PTW5s)
**Engine:** Clue as a predefined token and a trigger on the investigate
action.

### 7. A token copy of a permanent spell enters without being created, and Mister Fantastic still sees it

**Cards:** [Iron Man, Bleeding Edge], [Mister Fantastic, Reed Richards], [Sol Ring]
**Board:** P1 controls Iron Man, Bleeding Edge and Mister Fantastic, Reed
Richards, and has cast no artifact spell this turn.
**Does:** P1 casts Sol Ring. Iron Man's ability triggers and resolves; P1
copies Sol Ring. The copy resolves, then Sol Ring.
**Then:** As the copy resolves it becomes a token Sol Ring on the
battlefield, and Mister Fantastic triggers once. When the card Sol Ring
resolves, Mister Fantastic doesn't trigger. An ability that triggers on a
token being created would not have triggered for the copy.
**Because:** CR 707.10f, CR 608.3f: a copy of a permanent spell becomes a
token as it resolves and is not "created"; Mister Fantastic's trigger
reads tokens entering.
**Source:** [Marvel Spoilers Day 1 - Discussing The 70+ New Cards From The Upcoming Marvel Set](https://www.youtube.com/watch?v=2zTqj8cPqNk)
**Engine:** copying a spell (CR 707.10) and a trigger on tokens entering.

### 8. Clara Oswald makes a Doctor's trigger trigger twice even when that Doctor isn't a Time Lord

**Cards:** [Clara Oswald], [Doctor Strange, Surgeon]
**Board:** Commander, starting life 40. P1 is at 52 life and controls
Clara Oswald, Doctor Strange, Surgeon and one other creature, a 2/2.
**Does:** P1's beginning of combat step begins.
**Then:** Doctor Strange's ability triggers twice. Both resolve: P1's
creatures get +4/+4 and vigilance until end of turn, the 2/2 a 6/6.
**Because:** Clara's static ability applies to a triggered ability of any
Doctor P1 controls, and Doctor Strange is a Human Doctor Hero; Doctor's
companion's Time Lord requirement is only about choosing commanders (CR
702.124m). CR 603.2d: the ability triggers an additional time. CR 603.4:
each copy of the intervening "if" is checked as it triggers and as it
resolves; 52 is at least 10 more than 40.
**Source:** [TR&CI 225: How Does Doctor's Companion Work (Or Not) w/ Doctor Strange, Doctor (And Other Doctors)](https://www.youtube.com/watch?v=FU2rQmOx0GQ)
**Engine:** a beginning-of-combat trigger runs (Luminarch Aspirant); an
intervening "if" over life totals and an additional-trigger static are
missing.

### 9. A paradigm's delayed trigger outlives its card leaving exile, and copying it gives two copies that turn

**Cards:** [Decorum Dissertation], [Pull from Eternity], [Strionic Resonator]
**Board:** P1 resolved Decorum Dissertation on an earlier turn, so it is
in exile and P1 has its paradigm delayed trigger. P1 controls an untapped
Strionic Resonator and has {2} available.
**Does:** On P2's turn, P2 casts Pull from Eternity targeting the exiled
Decorum Dissertation; it goes to P1's graveyard. P1's next precombat main
phase begins; the delayed trigger triggers; P1 copies it with Strionic
Resonator.
**Then:** Both resolve: P1 creates two copies of Decorum Dissertation in
exile and may cast each without paying its mana cost (a target player
draws two and loses 2 life, twice). When each copy resolves P1 gets no
further delayed trigger, since a spell with that name has resolved for P1
before. At P1's following precombat main phase the delayed trigger fires
once again.
**Because:** CR 702.192a: the delayed trigger is created as the first
spell with that name resolves and lasts for the rest of the game; nothing
ties it to the card in exile (Improvisation Capstone's ruling says so).
CR 603.7a, CR 707.10: a copy of a triggered ability resolves like the
original.
**Source:** [TR&CI 219: Paradigm Deep Dive - A Comprehensive Guide Of Rules & Tricks You Can Pull Off](https://www.youtube.com/watch?v=yJD1U0EnhB8)
**Engine:** paradigm, a delayed trigger "for the rest of the game"
(Portent's triggers once), casting a copy from exile (CR 707.12) and
copying a triggered ability.
