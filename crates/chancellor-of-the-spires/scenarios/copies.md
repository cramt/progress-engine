# Copies

### 1. Thespian's Stage copying a creature that Imprisoned in the Moon made a land becomes the creature

**Cards:** [Imprisoned in the Moon], [Sisay, Weatherlight Captain], [Thespian's Stage]
**Board:** P2 controls Sisay, Weatherlight Captain, enchanted by P1's
Imprisoned in the Moon. P1 controls Thespian's Stage, untapped, and {2}
available, and no legendary permanents.
**Does:** P1 activates Thespian's Stage targeting Sisay; it resolves.
**Then:** The target was legal: Sisay is a colorless land. P1's Thespian's
Stage is now Sisay, Weatherlight Captain, a white legendary Human Soldier
creature 2/2 with Sisay's two abilities and "{2}, {T}: This land becomes a
copy of target land, except it has this ability", and is not a land. P2's
Sisay is still a colorless land with only "{T}: Add {C}". No legend rule
applies: the two have different controllers.
**Because:** CR 707.2: copiable values are those printed, modified only by
copy effects; the Aura's type-, colour- and ability-changing effects
(layers 4–6) aren't copied. CR 613.1a: the copy applies in layer 1.
CR 707.9a: the Stage keeps its ability as part of the copy. CR 704.5j: the
legend rule is per controller.
**Source:** [Copy Mechanisms #mtg](https://www.youtube.com/watch?v=eYfqXvYawQ4)
**Engine:** a permanent becoming a copy runs (Thespian's Stage, README,
*The milestone*), and Auras' layer effects run; missing are Sisay's "for
each color among other legendary permanents you control" and Imprisoned
in the Moon granting a mana ability alongside removing all others.

### 2. A copy of a copy copies what the first copy copied

**Cards:** [Phyrexian Metamorph], [Sol Ring], [Sculpting Steel]
**Board:** P2 controls Sol Ring. P1 has Phyrexian Metamorph in hand;
there are no creatures on the battlefield.
**Does:** P1 casts Phyrexian Metamorph; it enters as a copy of Sol Ring.
On P2's turn, P2 casts Sculpting Steel choosing P1's Metamorph.
**Then:** P1's Metamorph is an artifact named Sol Ring with "{T}: Add
{C}{C}" (its exception, "an artifact in addition to its other types",
adds nothing). P2's Sculpting Steel enters as Sol Ring too, not as a
Phyrexian Metamorph; it has no "enter as a copy" choice of its own to
make.
**Because:** CR 707.3: a copy's copiable values become the copied
information, and objects that copy it use those values. CR 707.9b: the
Metamorph's exception is part of its copiable values. CR 614.1c,
CR 707.2.
**Source:** [Copy Mechanisms #mtg](https://www.youtube.com/watch?v=eYfqXvYawQ4)
**Engine:** entering as a copy runs (Vesuva); Phyrexian mana ({U/P}) is
refused.

### 3. Parallel Lives tokens double each other's tokens but not their own creation

**Cards:** [Astral Dragon], [Parallel Lives], [Miirym, Sentinel Wyrm]
**Board:** P1 controls Parallel Lives and Miirym, Sentinel Wyrm, and no
other token doublers.
**Does:** P1 casts Astral Dragon; it resolves. Its Project Image trigger
(targeting Parallel Lives) and Miirym's trigger go on the stack, P1
putting Project Image on top. Project Image resolves, then Miirym's.
**Then:** After Project Image, P1 controls five Parallel Lives: the card
and four tokens, each a 3/3 Dragon creature enchantment with flying.
Miirym doesn't trigger for them: they are tokens. Miirym's trigger then
creates 32 token copies of Astral Dragon at once (one, doubled by each of
five replacement effects), and 32 Project Image abilities trigger.
**Because:** CR 614.1, CR 614.5, CR 616.1: each Parallel Lives is a
separate replacement effect, each applying once to the event, so one
token becomes 2^5. The four new tokens weren't on the battlefield when
their own creation was replaced. CR 603.2c, CR 603.6a: each Astral Dragon
token's own enters ability triggers.
**Source:** [Biggest Play In the Universe? #mtg](https://www.youtube.com/watch?v=eMJAoWnGbzU)
**Engine:** token copies "except" a type and flying, a replacement on
token creation (README: replacements are carried, not run), and
Miirym's "nontoken Dragon" trigger.
