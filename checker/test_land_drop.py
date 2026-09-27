"""The checker's declared land drop (`drop_line_path`), held to the hands in
HANDS.md whose numbers are settled on paper: hand 12 (which land the list
plays), hand 17 (Urza's Saga's third chapter, two turns after the drop), hand
42 (Expedition Map paid for before the drop that plays the Saga it found) and
hand 61 (Artificer's Intuition's discard, paid as a cost). Every deal of each
small deck is played - the opening seven as one unordered hand, the draws in
order - and weighed by how many orders of the
whole deck produce it, so the answers are exact fractions.

`checker/test_activation.py` plays hand 42 with a model of its own, written
for that hand alone; this plays it through the model the north-star question
uses, which knows nothing about the hand.

    python3 -m unittest discover -s checker
"""

from __future__ import annotations

import os
import sys
import unittest
from collections import Counter
from fractions import Fraction
from itertools import product
from math import comb, prod
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import checker  # noqa: E402

DECKS = Path(os.environ.get("CHECKER_DECKS", HERE.parent / "decks"))
INDEX = checker.Index(DECKS / "index.jsonl")
ISLAND, BOLT, SEWERS = "Island", "Lightning Bolt", "Undercity Sewers"
MAP, SAGA, LANTERN = "Expedition Map", "Urza's Saga", "Lantern of Insight"


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def deals(decklist: list[tuple[str, int]], draws: int, on_the_draw: bool):
    """(weight, game) for every opening seven and every ordered run of `draws`
    cards after it. The weights sum to one."""
    names = [n for n, _ in decklist]
    deck = Counter(dict(decklist))
    size = sum(deck.values())
    cards = {n: card(n) for n in names}
    library = [cards[n] for n, k in decklist for _ in range(k)]
    total = Fraction(0)
    for split in product(*(range(deck[n] + 1) for n in names)):
        if sum(split) != 7:
            continue
        opener = Counter(dict(zip(names, split)))
        p_open = Fraction(prod(comb(deck[n], opener[n]) for n in names), comb(size, 7))
        left = deck - opener
        for run in product(names, repeat=draws):
            remaining = Counter(left)
            p = p_open
            for d in run:
                if remaining[d] == 0:
                    p = Fraction(0)
                    break
                p *= Fraction(remaining[d], sum(remaining.values()))
                remaining[d] -= 1
            if p == 0:
                continue
            total += p
            dealt = [cards[n] for n in names for _ in range(opener[n])] + [cards[d] for d in run]
            yield p, checker.Game(dealt, on_the_draw, size, library=library)
    assert total == 1, total


def share(decklist, draws, on_the_draw, holds) -> Fraction:
    return sum((p for p, g in deals(decklist, draws, on_the_draw) if holds(g)), Fraction(0))


def _saga(c: checker.Card) -> bool:
    return c.name == SAGA


def _any_land(c: checker.Card) -> bool:
    return True


SAGA_FIRST = checker.LandDrop((_saga, _any_land))
CHAPTERS = {SAGA: (LANTERN,)}


class TheCardsReadThemselves(unittest.TestCase):
    def test_the_sagas_third_chapter_finds_the_lantern(self):
        self.assertTrue(checker.chapter_three_puts(card(SAGA), card(LANTERN)))
        # Mana cost {0} or {1}: Sol Ring is one, Wurmcoil Engine is not.
        self.assertTrue(checker.chapter_three_puts(card(SAGA), card("Sol Ring")))
        self.assertFalse(checker.chapter_three_puts(card(SAGA), card("Wurmcoil Engine")))

    def test_the_map_is_a_tap_and_sacrifice_search_for_a_land(self):
        self.assertEqual(checker.activated_search(card(MAP)), ("{2}", "land"))
        self.assertIsNone(checker.activated_search(card(LANTERN)))

    def test_the_hand_tutors_find_the_lantern_and_the_map_does_not(self):
        for tutor in ("Trinket Mage", "Fabricate", "Tezzeret, Cruel Captain"):
            self.assertTrue(checker.searches_to_hand(card(tutor), card(LANTERN)), tutor)
        # Trinket Mage finds mana value 1 or less; Sol Ring is 1, Wurmcoil 6.
        self.assertFalse(checker.searches_to_hand(card("Trinket Mage"), card("Wurmcoil Engine")))
        self.assertTrue(checker.searches_to_hand(card("Fabricate"), card("Wurmcoil Engine")))
        # The Map's search is its activation's, and it finds a land.
        self.assertFalse(checker.searches_to_hand(card(MAP), card(LANTERN)))
        self.assertFalse(checker.searches_to_hand(card("Trinket Mage"), card(ISLAND)))


class HandTwelve(unittest.TestCase):
    """Undercity Sewers, an Island, the Lantern and nine Bolts; the line casts
    the Lantern. Turn 1 on the play is the opening seven. Both of the hand's
    files route what the surveil sees, so a look is a deeper one."""

    DECK = [(ISLAND, 1), (BOLT, 9), (LANTERN, 1), (SEWERS, 1)]

    def cast_on_turn_1(self, drop: checker.LandDrop) -> Fraction:
        return share(
            self.DECK,
            0,
            False,
            lambda g: checker.drop_line_path(g, ((LANTERN,),), 1, drop).cast_by(LANTERN, 1),
        )

    def test_surveil_first_plays_the_sewers_whenever_it_is_held(self):
        drop = checker.LandDrop((lambda c: "surveil" in c.tags,), routed=True)
        self.assertEqual(self.cast_on_turn_1(drop), Fraction(126, 792))

    def test_untapped_first_plays_the_island(self):
        drop = checker.LandDrop((lambda c: not c.enters_tapped,), routed=True)
        self.assertEqual(self.cast_on_turn_1(drop), Fraction(252, 792))

    def test_a_list_naming_neither_breaks_the_tie_by_the_deeper_look(self):
        # One entry holding both lands: the Sewers looks one card deeper.
        drop = checker.LandDrop((_any_land,), routed=True)
        self.assertEqual(self.cast_on_turn_1(drop), Fraction(126, 792))

    def test_a_look_that_routes_nothing_breaks_no_tie(self):
        # Nothing routed, the tie goes to the decklist: the Island is named
        # before Undercity Sewers, so it is played whenever both are held.
        self.assertEqual(self.cast_on_turn_1(checker.LandDrop((_any_land,))), Fraction(252, 792))


class HandSeventeen(unittest.TestCase):
    """hand-saga.txt: the Saga, the Lantern, four Islands, ten Bolts; the Saga
    played the moment it is held, and no line."""

    DECK = [(ISLAND, 4), (BOLT, 10), (LANTERN, 1), (SAGA, 1)]

    def lantern_in_play(self, turn: int, on_the_draw: bool) -> Fraction:
        draws = turn if on_the_draw else turn - 1
        return share(
            self.DECK,
            draws,
            on_the_draw,
            lambda g: checker.drop_line_path(
                g, (), turn, SAGA_FIRST, chapters=CHAPTERS
            ).on_battlefield_by(LANTERN, turn),
        )

    def test_by_turn_3(self):
        self.assertEqual(self.lantern_in_play(3, False), Fraction(49, 240))  # 20.42%
        self.assertEqual(self.lantern_in_play(3, True), Fraction(1, 5))  # 20.00%

    def test_by_turn_5(self):
        self.assertEqual(self.lantern_in_play(5, False), Fraction(1, 4))  # 25.00%
        self.assertEqual(self.lantern_in_play(5, True), Fraction(19, 80))  # 23.75%


class HandFortyTwo(unittest.TestCase):
    """Five Islands, eight Bolts, the Map, the Saga and the Lantern, on the
    play; the line names only the Map."""

    DECK = [(ISLAND, 5), (BOLT, 8), (MAP, 1), (SAGA, 1), (LANTERN, 1)]

    def rows(self, fetches) -> dict[str, Fraction]:
        out = Counter()
        for p, g in deals(self.DECK, 5, False):
            path = checker.drop_line_path(
                g, ((MAP,),), 5, SAGA_FIRST, fetches=fetches, chapters=CHAPTERS
            )
            out["map_turn_1"] += p * path.cast_by(MAP, 1)
            out["lantern_by_5"] += p * path.on_battlefield_by(LANTERN, 5)
        return out

    def test_never_activated(self):
        got = self.rows({})
        self.assertEqual(got["map_turn_1"], Fraction(4921, 11440))
        self.assertEqual(got["lantern_by_5"], Fraction(1, 4))

    def test_activated_for_the_saga(self):
        got = self.rows({MAP: (SAGA,)})
        self.assertEqual(got["map_turn_1"], Fraction(4921, 11440))
        self.assertEqual(got["lantern_by_5"], Fraction(3977, 12870))


class HandSixtyOne(unittest.TestCase):
    """Ten Islands, Artificer's Intuition, Codex Shredder and the Lantern, on
    the play to turn 4; the line names the Lantern, then Intuition. The
    activation's discard is paid only with a card the [discard] list names."""

    INTUITION, SHREDDER = "Artificer's Intuition", "Codex Shredder"
    DECK = [(ISLAND, 10), (INTUITION, 1), (SHREDDER, 1), (LANTERN, 1)]

    def rows(self, fetches, discard) -> dict[str, Fraction]:
        out = Counter()
        line = ((LANTERN,), (self.INTUITION,))
        for p, g in deals(self.DECK, 3, False):
            path = checker.drop_line_path(
                g, line, 4, checker.LandDrop((_any_land,)), fetches=fetches, discard=discard
            )
            out["intuition_by_2"] += p * path.cast_by(self.INTUITION, 2)
            out["lantern_by_4"] += p * path.on_battlefield_by(LANTERN, 4)
        return out

    def test_the_intuition_is_read_from_its_text(self):
        a = checker.activation_of(card(self.INTUITION))
        self.assertEqual(
            (a.cost, a.discards, a.finds, a.most, a.sacrifice),
            ("{U}", "artifact", "artifact", 1, False),
        )

    def test_never_activated(self):
        got = self.rows({}, None)
        self.assertEqual(got["intuition_by_2"], Fraction(89, 156))
        self.assertEqual(got["lantern_by_4"], Fraction(10, 13))

    def test_the_shredder_pays_the_discard(self):
        shredder = lambda c: c.name == self.SHREDDER  # noqa: E731
        got = self.rows({self.INTUITION: (LANTERN,)}, shredder)
        self.assertEqual(got["intuition_by_2"], Fraction(89, 156))
        self.assertEqual(got["lantern_by_4"], Fraction(265, 286))

    def test_a_list_naming_no_artifact_card_pays_nothing(self):
        lands = lambda c: "Land" in c.type_line  # noqa: E731
        got = self.rows({self.INTUITION: (LANTERN,)}, lands)
        self.assertEqual(got["lantern_by_4"], Fraction(10, 13))


if __name__ == "__main__":
    unittest.main()
