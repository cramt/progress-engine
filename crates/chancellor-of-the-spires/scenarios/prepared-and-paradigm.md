# Prepared and paradigm

Both put a copy of a spell in exile to be cast later (CR 722, CR 702.192),
so they share the questions of what the copy is and who may cast it.

### 1. A creature that enters prepared lets its controller cast a sorcery copy before anyone can respond, unless a trigger opens a window

**Cards:** [Emeritus of Woe // Demonic Tutor], [Swords to Plowshares], [Ayara, First of Locthwain]
**Board:** P1's precombat main phase, empty stack. P2 has Swords to
Plowshares and {W} open.
**Does:** P1 casts Emeritus of Woe; it resolves and enters prepared.
**Then:** P1 receives priority with an empty stack and casts the copy of
Demonic Tutor from exile before P2 can cast Swords to Plowshares; Emeritus
is no longer prepared. Control: if P1 also controls Ayara, First of
Locthwain, Ayara's trigger is on the stack when P1 gets priority, so P1
can't cast the sorcery copy; P2 can exile Emeritus in response, and the
Demonic Tutor copy ceases to exist with it.
**Because:** CR 614.1d, CR 722.3a: entering prepared is part of the event
that puts it onto the battlefield, not a trigger. CR 117.3b: the active
player gets priority after a resolution. CR 307.1: a sorcery needs an
empty stack. CR 722.3c: the copy stays in exile only while the prepared
permanent remains on the battlefield.
**Source:** [TR&CI 218: Prepared Deep Dive - A Comprehensive Guide of 20+ Rules & Card Interactions w/ Prepared](https://www.youtube.com/watch?v=Hz8RnBvXXtQ)
**Engine:** prepared (`EntersPrepared` is carried unread today) and
casting a copy from exile (CR 707.12); an enters trigger on "another black
creature" is in the worklist (Ayara, "an enters trigger on SelfOrAnother").

### 2. The prepared spell is cast by whoever controls the creature now

**Cards:** [Emeritus of Conflict // Lightning Bolt], [Zealous Conscripts]
**Board:** P1 controls Emeritus of Conflict, prepared (a Lightning Bolt
copy in exile). P2's turn.
**Does:** P2 casts Zealous Conscripts, gaining control of the Emeritus
until end of turn.
**Then:** P2 may cast the Lightning Bolt copy; P1 may not while P2
controls it. Once P2 casts it, the Emeritus is unprepared, and stays so
when it returns to P1 at end of turn.
**Because:** CR 722.3c: "the prepared permanent's controller may cast the
copy", read at the time; Grave Researcher's ruling says ownership and who
controlled it when it became prepared don't matter.
**Source:** [TR&CI 218: Prepared Deep Dive - A Comprehensive Guide of 20+ Rules & Card Interactions w/ Prepared](https://www.youtube.com/watch?v=Hz8RnBvXXtQ)
**Engine:** prepared and control-changing effects (layer 2).

### 3. Prepared isn't copiable, losing all abilities doesn't unprepare, and the exiled copy doesn't follow a new copy effect

**Cards:** [Grave Researcher // Reanimate], [Emeritus of Woe // Demonic Tutor], [Clone], [Darksteel Mutation], [Mirrorform], [Skycoach Waypoint]
**Board:** P1 controls Grave Researcher, prepared (a Reanimate copy in
exile), Emeritus of Woe, not prepared, and Skycoach Waypoint.
**Does / Then:**
- P2 casts Clone copying the Grave Researcher: the Clone isn't prepared.
- P2 enchants the Grave Researcher with Darksteel Mutation: it stays
  prepared and P1 can still cast the Reanimate copy. After P1 casts it,
  Skycoach Waypoint's "{3}, {T}: Target creature becomes prepared" makes
  it prepared again with a new Reanimate copy, though it has no abilities.
- Instead, P1 casts Mirrorform targeting Emeritus of Woe while the
  Researcher is prepared: the Researcher becomes a copy of Emeritus of
  Woe, still prepared, and the copy in exile is still Reanimate.
- Skycoach Waypoint targeting a creature with no prepare spell (Llanowar
  Elves) does nothing.
**Because:** CR 722.3a: prepared is a designation, gained only by a
permanent with a prepare spell; CR 722.2a, CR 722.2b: the prepare spell's
characteristics are copiable values, not an ability, so Darksteel
Mutation doesn't remove them. CR 722.3c: the copy is made once, as the
permanent becomes prepared, and stays while it remains prepared. Grave
Researcher's rulings say each of these.
**Source:** [TR&CI 218: Prepared Deep Dive - A Comprehensive Guide of 20+ Rules & Card Interactions w/ Prepared](https://www.youtube.com/watch?v=Hz8RnBvXXtQ)
**Engine:** prepared, Auras setting type and removing abilities, and
Mirrorform's "each nonland permanent you control becomes a copy".

### 4. A prepared spell is named for its prepare spell, so Meddling Mage naming Reanimate stops it

**Cards:** [Meddling Mage], [Grave Researcher // Reanimate], [Reanimate]
**Board:** P2 controls Meddling Mage, which named Reanimate as it entered.
P1 controls Grave Researcher, prepared, and has Reanimate in hand.
**Does:** P1 tries to cast the Reanimate copy from exile, and Reanimate
from hand.
**Then:** Neither can be cast.
**Because:** CR 722.3c: the copy has only the prepare spell's
characteristics, its name Reanimate among them. CR 722.5: Meddling Mage
could name a prepare spell's name.
**Source:** [TR&CI 218: Prepared Deep Dive - A Comprehensive Guide of 20+ Rules & Card Interactions w/ Prepared](https://www.youtube.com/watch?v=Hz8RnBvXXtQ)
**Engine:** a card name chosen runs (Cabal Therapy); missing are "spells
with the chosen name can't be cast" and prepared.

### 5. A paradigm spell needs to resolve, and a copy you control resolving counts

**Cards:** [Decorum Dissertation], [Disdainful Stroke], [Reverberate]
**Board:** P1 and P2, neither having resolved a spell named Decorum
Dissertation. P1 has Disdainful Stroke, Reverberate, and enough mana for either.
**Does:** P2 casts Decorum Dissertation (mana value 5). (a) P1 counters it
with Disdainful Stroke. (b) Instead P1 casts Reverberate copying it; P1's
copy resolves, then P2's Decorum Dissertation.
**Then:** (a) P2 gets no paradigm delayed trigger; the card goes to P2's
graveyard. (b) Both players get one: from each of their next precombat
main phases on, each may cast a copy free. P1's copy ceases to exist in
exile, and P2's card is exiled.
**Because:** CR 702.192a: the delayed trigger is created only "if this is
the first time a spell you control with this spell's name has resolved
this game". CR 707.10: a copy of a spell resolves like one, under its
controller (Reverberate's caster). CR 707.10a: a spell copy outside the
stack ceases to exist.
**Source:** [TR&CI 219: Paradigm Deep Dive - A Comprehensive Guide Of Rules & Tricks You Can Pull Off](https://www.youtube.com/watch?v=yJD1U0EnhB8)
**Engine:** paradigm, copying a spell (CR 707.10), and Disdainful Stroke's
"with mana value 4 or greater" target filter.
