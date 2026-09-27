"""HANDS.md hand 42, held against a game played from the cards' text: Expedition
Map goes and gets Urza's Saga, and the Saga's third chapter gets the Lantern.

Written from the Comprehensive Rules and README/HANDS.md, never from crates/:

* Expedition Map reads "{2}, {T}, Sacrifice this artifact: Search your library
  for a land card, reveal it, put it into your hand, then shuffle." Everything
  before the colon is the cost (CR 602.1a), paid in full as the ability is
  activated (CR 602.2b, 601.2g-h), and sacrificing is part of that payment
  (CR 118.3): the Map is gone before the search resolves, so one Map is one
  activation, ever.
* {T} in a cost needs the permanent untapped, and only a creature waits out
  summoning sickness to pay it (CR 302.6). The Map is an artifact, so a Map
  cast this turn can be activated this turn.
* Urza's Saga gets a lore counter as it enters and after each of your draw
  steps (CR 714.2b, 714.3b), taps for {C} from chapter I, and chapter III
  searches for an artifact card with mana cost {0} or {1} and puts it onto the
  battlefield; the Saga is sacrificed once III has resolved (CR 714.4). The
  mana it made with III on the stack stays in the pool for that main phase.
* One land a turn (CR 305.2), at any point of a main phase the stack is empty
  (CR 505.6b), so paying for the Map before or after it is the pilot's order.

What a line is, and the one thing paid before the drop, is the README's and
ADR-0019's, marked ASSUMPTION below.

Sixteen cards on the play, to turn 5. The first seven are one unordered hand
and the next five are dealt in order: turn 5 sees four draws, and one Saga
fetched from among them moves a fifth card up. Every such deal is weighed by
how many orders of the whole deck produce it, so the answers are exact.

    python3 -m unittest discover -s checker
"""

from __future__ import annotations

import os
import re
import sys
import unittest
from collections import Counter
from fractions import Fraction
from itertools import product
from math import comb
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import checker  # noqa: E402

DECKS = Path(os.environ.get("CHECKER_DECKS", HERE.parent / "decks"))
INDEX = checker.Index(DECKS / "index.jsonl")
ISLAND, BOLT = "Island", "Lightning Bolt"
MAP, SAGA, LANTERN = "Expedition Map", "Urza's Saga", "Lantern of Insight"
DECK = Counter({ISLAND: 5, BOLT: 8, MAP: 1, SAGA: 1, LANTERN: 1})


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


_ACTIVATED_SEARCH = re.compile(
    r"^((?:\{\d+\})+), \{T\}, Sacrifice this artifact: Search your library for an? (\w+) card, "
    r"reveal it, put it into your hand",
    re.M,
)


def activation(c: checker.Card) -> tuple[int, str]:
    """(generic mana, card type it finds) from an artifact's tap-and-sacrifice
    search, read off its text (CR 602.1a: the cost is what precedes the
    colon)."""
    m = _ACTIVATED_SEARCH.search(c.oracle)
    if not m:
        raise ValueError(f"{c.name} has no tap-and-sacrifice search")
    generic, pips = checker.parse_cost(m.group(1))
    assert not pips, "a generic cost, which any land pays"
    return generic, m.group(2).lower()


def play(
    opener: Counter, draws: list[str], rest: Counter, activate: bool, before_drop: bool = True
) -> dict:
    """One game of hand 42's line, [casting] prefer = [Map], [land_drop]
    prefer = [Saga, t:land]. Returns what the three rows ask."""
    map_cost = checker.parse_cost(card(MAP).mana_cost)[0]
    act_cost, finds = activation(card(MAP))
    assert finds == "land" and "Land" in card(SAGA).type_line
    hand = Counter(opener)
    top = list(draws)
    library_rest = Counter(rest)
    islands = 0
    saga_lore = None  # lore counters on the Saga in play, None if not in play
    maps_in_play = 0  # untapped, unsacrificed Maps
    out = {"map_turn_1": False, "saga_by_3": False, "lantern_by_5": False}
    lantern_in_play = False

    def in_library(name: str) -> bool:
        return name in top or library_rest[name] > 0

    def take(name: str) -> None:
        if name in top:
            top.remove(name)
        else:
            library_rest[name] -= 1

    for turn in range(1, 6):
        if turn > 1:
            hand[top.pop(0)] += 1  # on the play, turn 1 draws nothing
        floating = 0
        if saga_lore is not None:
            saga_lore += 1  # CR 714.3b: after the draw step
            if saga_lore == 3:
                # Chapter III on the stack: tap the Saga for {C} first, then
                # it resolves and the Saga is sacrificed (CR 714.4).
                floating = 1
                if in_library(LANTERN):
                    take(LANTERN)
                    lantern_in_play = True
                saga_lore = None
        saga_mana = 1 if saga_lore is not None else 0
        spent = 0
        # ASSUMPTION (ADR-0019, HANDS.md hand 42): the one thing paid before
        # the land drop is an activation whose search finds a land the
        # declared drop ranks above every land in hand. Here: the Map, for
        # the Saga, when the hand holds no Saga. Paid from what is in play.
        if before_drop and activate and maps_in_play and hand[SAGA] == 0 and in_library(SAGA):
            if islands + saga_mana + floating - spent >= act_cost:
                spent += act_cost
                maps_in_play -= 1
                take(SAGA)
                hand[SAGA] += 1
        # The land drop: the Saga first, then an Island.
        if hand[SAGA]:
            hand[SAGA] -= 1
            saga_lore = 1  # CR 714.3a: it enters with one; chapter I
            saga_mana = 1
            if turn <= 3:
                out["saga_by_3"] = True
        elif hand[ISLAND]:
            hand[ISLAND] -= 1
            islands += 1
        # ASSUMPTION (README "[casting]", ADR-0019): the line walks its list
        # after the drop and does the first thing the pool still pays for —
        # activate a Map already in play, else cast one from hand — and reads
        # the list again from the top after each. A declared line activates
        # whenever the pool pays, even with nothing left to find.
        while True:
            left = islands + saga_mana + floating - spent
            if activate and maps_in_play and left >= act_cost:
                spent += act_cost
                maps_in_play -= 1
                if in_library(SAGA):
                    take(SAGA)
                    hand[SAGA] += 1
                continue
            if hand[MAP] and left >= map_cost:
                spent += map_cost
                hand[MAP] -= 1
                maps_in_play += 1
                if turn == 1:
                    out["map_turn_1"] = True
                continue
            break
    out["lantern_by_5"] = lantern_in_play
    return out


def shares(activate: bool, before_drop: bool = True) -> dict[str, Fraction]:
    names = sorted(DECK)
    total = Fraction(0)
    hits = Counter()
    for split in product(*(range(DECK[n] + 1) for n in names)):
        if sum(split) != 7:
            continue
        opener = Counter(dict(zip(names, split)))
        p_open = Fraction(_prod(comb(DECK[n], opener[n]) for n in names), comb(16, 7))
        left = DECK - opener
        for draws in product(names, repeat=5):
            remaining = Counter(left)
            p = p_open
            for d in draws:
                if remaining[d] == 0:
                    p = Fraction(0)
                    break
                p *= Fraction(remaining[d], sum(remaining.values()))
                remaining[d] -= 1
            if p == 0:
                continue
            total += p
            for row, held in play(opener, list(draws), remaining, activate, before_drop).items():
                if held:
                    hits[row] += p
    assert total == 1, total
    return {row: hits[row] for row in ("map_turn_1", "saga_by_3", "lantern_by_5")}


def _prod(xs) -> int:
    r = 1
    for x in xs:
        r *= x
    return r


class HandFortyTwo(unittest.TestCase):
    def test_the_map_is_read_from_its_text(self):
        self.assertEqual(checker.parse_cost(card(MAP).mana_cost), (1, []))
        self.assertEqual(activation(card(MAP)), (2, "land"))
        self.assertIn("Artifact", card(MAP).type_line)
        self.assertNotIn("Creature", card(MAP).type_line)

    def test_never_activated(self):
        got = shares(activate=False)
        self.assertEqual(got["map_turn_1"], Fraction(4921, 11440))
        self.assertEqual(got["saga_by_3"], Fraction(9, 16))
        self.assertEqual(got["lantern_by_5"], Fraction(1, 4))

    def test_activated_for_the_saga(self):
        got = shares(activate=True)
        self.assertEqual(got["map_turn_1"], Fraction(4921, 11440))
        self.assertEqual(got["saga_by_3"], Fraction(26249, 34320))
        self.assertEqual(got["lantern_by_5"], Fraction(3977, 12870))

    def test_the_exception_is_what_makes_the_saga_that_turns_land(self):
        # Paid only after the drop, a Map cast on turn 2 and activated on
        # turn 3 finds a Saga that waits for turn 4: less on both rows.
        after = shares(activate=True, before_drop=False)
        self.assertLess(after["saga_by_3"], Fraction(26249, 34320))
        self.assertLess(after["lantern_by_5"], Fraction(3977, 12870))
        self.assertEqual(after["map_turn_1"], Fraction(4921, 11440))


if __name__ == "__main__":
    unittest.main()
