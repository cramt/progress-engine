"""HANDS.md hand 40, held against the checker's line: Tezzeret the Seeker puts
the Lantern onto the battlefield with a loyalty ability, the turn he is cast.

Ten Islands, the Seeker and the Lantern, on the play. Every deal is one of 132
placements of the two non-Islands among twelve positions, all equally likely,
so every one is played and the answers are exact fractions.

    python3 -m unittest discover -s checker
"""

from __future__ import annotations

import os
import sys
import unittest
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import checker  # noqa: E402

DECKS = Path(os.environ.get("CHECKER_DECKS", HERE.parent / "decks"))
INDEX = checker.Index(DECKS / "index.jsonl")
SEEKER, LANTERN = "Tezzeret the Seeker", "Lantern of Insight"


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def deals():
    island, seeker, lantern = card("Island"), card(SEEKER), card(LANTERN)
    for s in range(12):
        for l in range(12):
            if s == l:
                continue
            cards = [island] * 12
            cards[s], cards[l] = seeker, lantern
            yield checker.Game(cards, False, 12, library=cards)


def share(line, puts, holds) -> Fraction:
    hits = total = 0
    for g in deals():
        total += 1
        hits += holds(checker.line_path(g, line, 5, puts=puts))
    return Fraction(hits, total)


LINE = ((LANTERN,), (SEEKER,))


class Loyalty(unittest.TestCase):
    """What the rules make of the Seeker's card, read from its oracle text."""

    def test_the_seeker_can_put_the_lantern_down_the_turn_he_enters(self):
        # Four loyalty (CR 306.5b), and −X with X = 1 pays 1 of it (CR 606.4),
        # for an artifact of mana value 1 or less: the Lantern.
        self.assertTrue(checker.puts_onto_battlefield(card(SEEKER), card(LANTERN)))

    def test_but_not_a_land_or_something_he_cannot_afford(self):
        self.assertFalse(checker.puts_onto_battlefield(card(SEEKER), card("Island")))
        # Mana value 5 would need −5, which four loyalty cannot pay.
        self.assertFalse(checker.puts_onto_battlefield(card(SEEKER), card("Wurmcoil Engine")))
        # And a card with no such ability puts nothing anywhere.
        self.assertFalse(checker.puts_onto_battlefield(card("Trinket Mage"), card(LANTERN)))


class HandForty(unittest.TestCase):
    def test_no_fetch(self):
        self.assertEqual(
            share(LINE, None, lambda p: p.cast_by(SEEKER, 5)), Fraction(37, 44)
        )
        self.assertEqual(
            share(LINE, None, lambda p: p.on_battlefield_by(LANTERN, 5)), Fraction(11, 12)
        )
        self.assertEqual(share(LINE, None, lambda p: p.cast_by(LANTERN, 5)), Fraction(11, 12))
        self.assertEqual(
            share(LINE, None, lambda p: p.in_library(LANTERN, 5)), Fraction(1, 12)
        )

    def test_fetch_to_battlefield(self):
        puts = {SEEKER: (LANTERN,)}
        self.assertEqual(share(LINE, puts, lambda p: p.cast_by(SEEKER, 5)), Fraction(37, 44))
        self.assertEqual(share(LINE, puts, lambda p: p.on_battlefield_by(LANTERN, 5)), 1)
        # A Lantern put there was never cast.
        self.assertEqual(share(LINE, puts, lambda p: p.cast_by(LANTERN, 5)), Fraction(11, 12))
        self.assertEqual(share(LINE, puts, lambda p: p.in_library(LANTERN, 5)), 0)

    def test_hand_44_the_lantern_out_of_the_line(self):
        # Only the Seeker puts it there: on turn 5, when it is the twelfth card.
        line = ((SEEKER,),)
        puts = {SEEKER: (LANTERN,)}
        self.assertEqual(share(line, puts, lambda p: p.on_battlefield_by(LANTERN, 4)), 0)
        self.assertEqual(
            share(line, puts, lambda p: p.on_battlefield_by(LANTERN, 5)), Fraction(1, 12)
        )
        self.assertEqual(share(line, puts, lambda p: p.in_library(LANTERN, 5)), 0)


if __name__ == "__main__":
    unittest.main()
