# Layers

### 1. An Aura removing all abilities beats a static that granted double strike before it

**Cards:** [Kenrith's Transformation], [Colossal Dreadmaw], [Berserkers' Onslaught], [Wall of Omens], [Duelist's Heritage]
**Board:** P1 controls Berserkers' Onslaught and Colossal Dreadmaw, Onslaught
on the battlefield first. P2 has enchanted the Dreadmaw with Kenrith's
Transformation. P2 controls Wall of Omens. P1's turn.
**Does:** P1 attacks P2 with the Dreadmaw. P2 blocks with Wall of Omens.
Combat damage.
**Then:** The Dreadmaw is a green Elk creature 3/3 with no abilities and
deals 3 damage once, in the regular combat damage step; Wall of Omens
(0/4) survives with 3 damage marked. Control: if P1 instead controls
Duelist's Heritage and gives the Dreadmaw double strike with its attack
trigger, that effect's timestamp is later than the Aura's and the Elk
deals 3 first-strike and 3 regular damage, destroying the Wall.
**Because:** CR 613.1f, CR 613.7: within layer 6 effects apply in
timestamp order. CR 613.7a: a static ability's effect has its object's
timestamp, so Onslaught's grant applies before the Aura's (CR 613.7e)
removal, whatever happened since. CR 613.7b: a resolving ability's effect
gets its timestamp as it is created, after the Aura's.
**Source:** [What time is it? #mtg](https://www.youtube.com/watch?v=gP888gtV1Q4)
**Engine:** layer 6 granting and removing in timestamp order and the
first-strike step run; missing are Auras setting a creature's type and
base power and toughness, a static over "attacking creatures", and Duelist's
Heritage's "whenever one or more creatures attack" trigger.

### 2. A type-setting "is a Werewolf" wipes the every-creature-type an Equipment gave earlier

**Cards:** [Werewolf Pack Leader], [Runed Stalactite]
**Board:** P1 controls Werewolf Pack Leader equipped with Runed Stalactite,
equipped before the activation below.
**Does:** P1 activates Werewolf Pack Leader's last ability ({3}{G}); it
resolves.
**Then:** Until end of turn the Pack Leader is a Werewolf and no other
creature type, 6/4 (base 5/3 plus the Stalactite's +1/+1), with trample.
At cleanup it is a 4/4 with every creature type again.
**Because:** CR 205.1a: an effect that sets a subtype replaces the
existing subtypes of that set. CR 613.1d, CR 613.7: the Stalactite's
every-creature-type (timestamp of its attachment, CR 613.7e) applies
first, then the later activation's effect. CR 613.4b, CR 613.4c: base 5/3
is set in 7b, the +1/+1 is modified in 7c. The video discusses this
wording as an Oracle change then coming; the "every type except Human"
interaction it mourns is gone from the current text.
**Source:** [Werewolf “Pack it up” Leader #mtg](https://www.youtube.com/watch?v=iFT_loe4t6o)
**Engine:** layer 4 setting a creature type (only basic land types are
set today), "is every creature type", and Equipment granting a static to
what it equips (equip itself runs, Leyline Axe).
