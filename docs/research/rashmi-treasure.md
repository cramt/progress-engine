# Rashmi's Treasure: measured, and not worth the width (#81)

**Question.** Once Rashmi and Ragavan is on the battlefield, the first spell
you cast each of your turns makes a Treasure. How much does counting that
Treasure move the Lantern north star, *Lantern of Insight on the battlefield
and Rashmi cast, by turn 5, one mana budget*?

**Answer.** Under 0.4 points at turn 5 in both seats, on every line measured.
The bar for building was 0.5. **Recommendation: close #81 as not worth the
width.** The free cast off the opponent's exiled card was out of scope and is
not counted.

> **Reproducing.** The measurement ran against the checker as it stood then,
> whose `line_path` took `treasure=`, `delayed=` and `treasure_ceiling=`. Those
> were measuring tools, not rules the engine has, and were left out of
> `checker/` when the mills (#85) reshaped `line_path`. That version is kept as
> `rashmi-treasure-checker.py`, and `rashmi-treasure.py` loads it.

## What was measured

`docs/research/rashmi-treasure.py` plays 200,000 deals of `decks/lantern.txt`
per seat (seed `81`) through the independent checker's line model
(`checker/checker.py`, `line_path`), which reads nothing in `crates/`. Each
deal is played with and without the Treasure. The difference is paired deal
by deal, and the interval is 95% on that paired difference.

The line has ADR 0018's rocks (Sol Ring, Arcane Signet, the three Talismans,
Mind Stone), and every Lantern route the checker models:

- the Lantern cast from hand;
- Trinket Mage fetching it to hand;
- Tezzeret the Seeker's −1 putting it onto the battlefield (ADR 0019, HANDS.md hand 40);
- Urza's Saga's chapter III, two turns after the Saga is played, with the Saga played the turn it is first held.

**The rules the checker learned**, from the card and the CR rather than from
`crates/`. They are in the line notes of `checker/checker.py` and pinned by
`the tests that shipped with the measurement`:

- **A trigger needs its permanent on the battlefield** (CR 603.2, 603.6). Casting Rashmi never triggers her. On the turn she resolves, that turn's first spell is behind her, so her first Treasure comes with the first spell of a *later* turn.
- **The Treasure is created after the first spell is cast** (CR 603.3). It pays for a later spell that turn, or on a later turn, and never for the spell that made it (CR 601.2g-h).
- **A Treasure is one mana of any colour, once** (CR 111.10a). It is kept across turns until spent, and it is not summoning-sick. The pilot spends one only when the lands and rocks cannot pay without it.

## Numbers

North star = Lantern on the battlefield **and** Rashmi cast. 200,000 deals
per seat.

| Line | Seat | By turn 5: without → with | Δ turn 5 | Δ turn 6 |
|---|---|---|---|---|
| Lantern, Rashmi, rocks, Trinket Mage, Seeker | play | 19.470% → 19.665% | **+0.196 ± 0.019** | +0.090 ± 0.013 |
| | draw | 24.785% → 25.055% | **+0.270 ± 0.023** | +0.095 ± 0.014 |
| Rashmi first, then the same | play | identical to the above for the north star | +0.196 ± 0.019 | +0.090 ± 0.013 |
| | draw | | +0.270 ± 0.023 | +0.095 ± 0.014 |
| Lantern first, then the ten one-mana permanents no route names | play | 19.470% → 19.733% | **+0.263 ± 0.022** | +0.295 ± 0.024 |
| | draw | 24.785% → 25.152% | **+0.367 ± 0.027** | +0.323 ± 0.025 |
| Ceiling: a free spell opens every turn after hers | play | 19.470% → 20.651% | +1.181 ± 0.047 | +1.162 ± 0.047 |
| | draw | 24.785% → 26.090% | +1.305 ± 0.050 | +1.139 ± 0.047 |

- **The Treasure never loses a game.** Across every row, no deal that reached the north star without it failed with it.
- **Rashmi cast by turn 5 does not move at all, as it must not.** Nothing she makes exists before she is cast. Her cast rate by turn 5 is 65.4% on the play and 73.4% on the draw.
- The one-mana permanents in the third line are Codex Shredder, Dakra Mystic, Expedition Map, Field of Dreams, Ghost Vacuum, Ghoulcaller's Bell, Pyxis of Pandemonium, Relic of Progenitus, Sensei's Divining Top and Soldier of Fortune.

## Why so little

- **The Treasure needs a cheap Rashmi.** It pays only after Rashmi has been cast on an earlier turn and a spell has been cast since. By turn 5 that means Rashmi on turn 3 or 4, and then a second spell on turn 4 or 5 that is short exactly one mana. Most north-star games are decided by drawing or finding the Lantern, not by that last mana. ADR 0018 found the same thing of the rocks.
- **The ceiling is not a line anyone can play.** It is the Treasure arriving before the turn's first spell. The deck has no zero-cost spell, so a one-drop cast only to trigger her costs the mana the Treasure gives back.
- **The one real extra is banking a spare mana.** On a turn when the line casts nothing else, a spare mana spent on a one-drop becomes a Treasure for later. The fillers line measures exactly that, and it still reads +0.26 and +0.37.
- **Turn 6 moves less than turn 5 on the plain line.** Most of the games the Treasure wins at turn 5 are games the sixth land wins anyway at turn 6.

## What building it would have cost

The Treasure is state carried across turns: how many are held and unspent.
That is a function of the path, so it adds no group of its own. But its
availability depends on the order of casts within a turn, and on whether
Rashmi was cast on an earlier turn. So every bill after hers would need the
nested-supply flow of ADR 0018 rather than the plain matching, and both
engines and the checker would carry a one-shot source. A fifth of a point to
a third of a point does not pay for that.

## If it is reopened

Reopen it if the north star moves to a question where one mana after the
commander is the bottleneck: a more expensive second payload, or a deck with
free spells. The checker already models the Treasure, behind `line_path(...,
treasure=...)`, and `the tests that shipped with the measurement` holds its timing. Re-run:

    python3 docs/research/rashmi-treasure.py --games 200000

It took 10m52s on four cores.
