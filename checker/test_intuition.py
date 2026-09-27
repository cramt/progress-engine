"""HANDS.md hand 61, held against a game played from the cards' text: Artificer's
Intuition discards an artifact card and finds the Lantern.

Written from the Comprehensive Rules and README/HANDS.md, never from crates/:

* Artificer's Intuition reads "{U}, Discard an artifact card: Search your
  library for an artifact card with mana value 1 or less, reveal it, put it
  into your hand, then shuffle." Everything before the colon is the cost
  (CR 602.1a), and the discard is part of it (CR 118.3, 701.9): paid in full
  as the ability is activated (CR 602.2b, 601.2g-h), before the search
  resolves. With no artifact card in hand the cost cannot be paid, so the
  ability cannot be activated (CR 602.2b, 601.2h) - unlike a spell's "discard"
  effect, which resolves with whatever there is.
* Its cost has no {T}, so the enchantment needs no untapping, no summoning
  sickness applies, and it could be activated again in the same turn. The
  shuffle leaves the rest of the library in a uniformly random order, which a
  fixed order with the found card taken out is as well.
* One land a turn (CR 305.2).

What a line is, and what [discard] prefer means for a cost, is the README's
and ADR-0019's, marked ASSUMPTION below.

Thirteen cards on the play, to turn 4, every order of the library at once: the
positions of the three or four named cards among thirteen, each as likely.

    python3 -m unittest discover -s checker
"""

from __future__ import annotations

import os
import re
import sys
import unittest
from collections import Counter
from fractions import Fraction
from itertools import permutations
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import checker  # noqa: E402

DECKS = Path(os.environ.get("CHECKER_DECKS", HERE.parent / "decks"))
INDEX = checker.Index(DECKS / "index.jsonl")
ISLAND = "Island"
INTUITION, SHREDDER, MAP, LANTERN = (
    "Artificer's Intuition",
    "Codex Shredder",
    "Expedition Map",
    "Lantern of Insight",
)


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


_ACTIVATED_SEARCH = re.compile(
    r"^((?:\{[0-9WUBRGC]\})+), Discard an? (\w+) card: Search your library for an? (\w+) card "
    r"with mana value (\d+) or less, reveal it, put it into your hand",
    re.M,
)


def activation(c: checker.Card) -> tuple[str, str, str, int]:
    """(mana cost, card type discarded, card type found, highest mana value
    found) off the text: the cost is what precedes the colon (CR 602.1a)."""
    m = _ACTIVATED_SEARCH.search(c.oracle)
    if not m:
        raise ValueError(f"{c.name} has no discard-and-search ability")
    return m.group(1), m.group(2).lower(), m.group(3).lower(), int(m.group(4))


def is_a(c: checker.Card, kind: str) -> bool:
    return kind.capitalize() in c.type_line


def play(order: list[str], listed: tuple[str, ...], activate: bool, first: str) -> dict:
    """One game of hand 61's line, [casting] prefer = [Lantern, Intuition], on
    the play to turn 4, the library in `order`, top first. `listed` is what
    [discard] prefer names; `first` is which of two listed cards goes when the
    hand holds both, which the caller weighs at a half each."""
    cast_cost, discards, finds, max_mv = activation(card(INTUITION))
    intuition_cost = card(INTUITION).mana_cost
    lantern_cost = card(LANTERN).mana_cost
    assert discards == finds == "artifact"
    assert is_a(card(LANTERN), finds) and checker._mana_value(card(LANTERN)) <= max_mv
    assert not is_a(card(INTUITION), "creature") and not is_a(card(INTUITION), "land")
    hand = Counter(order[:7])
    library = list(order[7:])
    islands = 0
    in_play = Counter()
    yard = Counter()
    out = {"intuition_by_2": False, "lantern_by_4": False}
    for turn in range(1, 5):
        if turn > 1:
            hand[library.pop(0)] += 1  # on the play, turn 1 draws nothing
        if hand[ISLAND]:
            hand[ISLAND] -= 1
            islands += 1
        left = islands  # every Island pays {U} or {1}
        activated = 0
        # ASSUMPTION (README "[casting]", ADR-0019): the line walks its list
        # and does the first thing the pool still pays for, reading it again
        # from the top after each: the Lantern; then, in Intuition's entry, an
        # activation of a copy in play and after it a cast from hand.
        while True:
            if hand[LANTERN] and left >= checker.parse_cost(lantern_cost)[0] + len(
                checker.parse_cost(lantern_cost)[1]
            ):
                left -= 1
                hand[LANTERN] -= 1
                in_play[LANTERN] += 1
                continue
            # ASSUMPTION (ADR-0019 §4, HANDS.md hand 61): the discard is paid
            # only with a card [discard] prefer names, and a line activates a
            # permanent at most once a turn, as it does a tapping one.
            payers = [n for n in listed if hand[n] and is_a(card(n), discards)]
            if activate and in_play[INTUITION] > activated and payers and left >= 1:
                assert cast_cost == "{U}"
                left -= 1
                activated += 1
                gone = first if first in payers else payers[0]
                hand[gone] -= 1
                yard[gone] += 1
                if LANTERN in library:
                    library.remove(LANTERN)
                    hand[LANTERN] += 1
                continue
            generic, pips = checker.parse_cost(intuition_cost)
            if hand[INTUITION] and left >= generic + len(pips):
                left -= generic + len(pips)
                hand[INTUITION] -= 1
                in_play[INTUITION] += 1
                if turn <= 2:
                    out["intuition_by_2"] = True
                continue
            break
    out["lantern_by_4"] = in_play[LANTERN] > 0
    out["shredder_in_yard"] = yard[SHREDDER] > 0
    out["map_in_yard"] = yard[MAP] > 0
    out["artifact_in_yard"] = yard[SHREDDER] + yard[MAP] > 0
    return out


def shares(named: list[str], islands: int, listed: tuple[str, ...], activate: bool) -> dict:
    size = len(named) + islands
    total = Fraction(0)
    hits: Counter = Counter()
    for spots in permutations(range(size), len(named)):
        order = [ISLAND] * size
        for name, at in zip(named, spots):
            order[at] = name
        both_listed = [n for n in (SHREDDER, MAP) if n in named and n in listed]
        firsts = both_listed if len(both_listed) == 2 else [SHREDDER]
        for first in firsts:
            w = Fraction(1, len(firsts))
            total += w
            for row, held in play(order, listed, activate, first).items():
                if held:
                    hits[row] += w
    count = total
    return {row: hits[row] / count for row in hits} | {
        row: Fraction(0)
        for row in ("intuition_by_2", "lantern_by_4", "shredder_in_yard", "map_in_yard")
        if row not in hits
    }


HAND_61 = [INTUITION, SHREDDER, LANTERN]


class HandSixtyOne(unittest.TestCase):
    def test_intuition_is_read_from_its_text(self):
        self.assertEqual(activation(card(INTUITION)), ("{U}", "artifact", "artifact", 1))
        self.assertEqual(checker.parse_cost(card(INTUITION).mana_cost), (1, ["U"]))
        self.assertIn("Enchantment", card(INTUITION).type_line)
        self.assertIn("Artifact", card(SHREDDER).type_line)

    def test_never_activated(self):
        got = shares(HAND_61, 10, (), activate=False)
        self.assertEqual(got["intuition_by_2"], Fraction(89, 156))
        self.assertEqual(got["lantern_by_4"], Fraction(10, 13))
        self.assertEqual(got["shredder_in_yard"], 0)

    def test_the_shredder_pays_and_the_lantern_is_found(self):
        got = shares(HAND_61, 10, (SHREDDER,), activate=True)
        self.assertEqual(got["intuition_by_2"], Fraction(89, 156))
        self.assertEqual(got["lantern_by_4"], Fraction(265, 286))
        self.assertEqual(got["shredder_in_yard"], Fraction(15, 26))

    def test_a_list_naming_no_artifact_never_pays(self):
        got = shares(HAND_61, 10, (ISLAND,), activate=True)
        self.assertEqual(got["lantern_by_4"], Fraction(10, 13))
        self.assertEqual(got["shredder_in_yard"], 0)

    def test_a_tie_in_the_list_falls_either_way_alike(self):
        got = shares([INTUITION, SHREDDER, MAP, LANTERN], 9, (SHREDDER, MAP), activate=True)
        self.assertEqual(got["artifact_in_yard"], Fraction(105, 143))
        self.assertEqual(got["shredder_in_yard"], got["map_in_yard"])
        self.assertEqual(got["shredder_in_yard"], Fraction(801, 1430))


if __name__ == "__main__":
    unittest.main()
