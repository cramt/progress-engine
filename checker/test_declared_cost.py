"""HANDS.md hands 41 and 50, held against the checker's line: a card played for
what the pilot pays rather than what is printed on it.

Hand 41 is Dizzy Spell's transmute (CR 702.53): "Transmute {1}{U}{U}" means
"{1}{U}{U}, Discard this card: Search your library for a card with the same
mana value as the discarded card, reveal that card, and put it into your hand.
Then shuffle. Activate only as a sorcery." Dizzy Spell's printed {U} makes its
mana value 1, so the transmute finds the Lantern, and costs three.

Hand 50 is Whir of Invention at X = 1 (CR 107.3a: the caster chooses X as the
spell is cast, and it is paid as that number): {1}{U}{U}{U}, and "an artifact
card with mana value X or less" onto the battlefield is the Lantern.

Ten Islands and two other cards, on the play: every deal is one of 132
placements of the two among twelve positions, all equally likely, so every
one is played and the answers are exact fractions.

    python3 -m unittest discover -s checker
"""

from __future__ import annotations

import itertools
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
DIZZY, WHIR, LANTERN = "Dizzy Spell", "Whir of Invention", "Lantern of Insight"


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def share(other, line, holds, turn, lands=None, **kw) -> Fraction:
    """The share of deals of `other`, the Lantern and `lands` (ten Islands
    unless said otherwise) on which `holds` of the line played to `turn`.
    Every placement of the two, and every distinct order of the lands, so the
    deals are equally likely."""
    lands = lands or ["Island"] * 10
    orders = sorted(set(itertools.permutations(lands))) if len(set(lands)) > 1 else [tuple(lands)]
    hits = total = 0
    for a in range(12):
        for b in range(12):
            if a == b:
                continue
            for order in orders:
                slots = iter(order)
                cards = [None] * 12
                cards[a], cards[b] = card(other), card(LANTERN)
                cards = [c if c is not None else card(next(slots)) for c in cards]
                g = checker.Game(cards, False, 12, library=cards)
                total += 1
                hits += holds(checker.line_path(g, line, turn, **kw))
    return Fraction(hits, total)


class Transmute(unittest.TestCase):
    """What CR 702.53 makes of Dizzy Spell's card, read from its oracle text."""

    def test_the_transmute_costs_what_it_says_and_finds_mana_value_one(self):
        dizzy = card(DIZZY)
        self.assertEqual(checker.play_cost(dizzy, checker.TRANSMUTE), "{1}{U}{U}")
        self.assertTrue(checker.transmute_finds(dizzy, card(LANTERN)))
        # The same mana value, not "or less": a land is mana value 0.
        self.assertFalse(checker.transmute_finds(dizzy, card("Island")))
        # And a card with no transmute has none to play.
        with self.assertRaises(ValueError):
            checker.play_cost(card("Trinket Mage"), checker.TRANSMUTE)


class HandFortyOne(unittest.TestCase):
    LINE = ((LANTERN,), (DIZZY,))
    FETCHES = {DIZZY: (LANTERN,)}

    def test_billed_at_the_printed_blue(self):
        # A Dizzy Spell cast for {U} that found the Lantern: the confident
        # wrong number the declared cost exists to catch.
        f = dict(fetches=self.FETCHES)
        self.assertEqual(share(DIZZY, self.LINE, lambda p: p.cast_by(DIZZY, 2), 4, **f), Fraction(2, 3))
        self.assertEqual(share(DIZZY, self.LINE, lambda p: p.cast_by(LANTERN, 2), 4, **f), Fraction(10, 11))
        self.assertEqual(share(DIZZY, self.LINE, lambda p: p.cast_by(LANTERN, 4), 4, **f), Fraction(65, 66))

    def test_billed_at_the_transmute(self):
        f = dict(fetches=self.FETCHES, modes={DIZZY: checker.TRANSMUTE})
        self.assertEqual(share(DIZZY, self.LINE, lambda p: p.cast_by(DIZZY, 2), 4, **f), 0)
        self.assertEqual(share(DIZZY, self.LINE, lambda p: p.cast_by(LANTERN, 2), 4, **f), Fraction(2, 3))
        self.assertEqual(share(DIZZY, self.LINE, lambda p: p.cast_by(LANTERN, 4), 4, **f), Fraction(65, 66))


class HandFifty(unittest.TestCase):
    LINE = ((LANTERN,), (WHIR,))
    PLAY = dict(puts={WHIR: (LANTERN,)}, modes={WHIR: checker.x_is(1)})

    def test_x_is_what_the_pilot_pays(self):
        whir = card(WHIR)
        self.assertEqual(checker.play_cost(whir, checker.x_is(1)), "{1}{U}{U}{U}")
        self.assertTrue(checker.puts_onto_battlefield(whir, card(LANTERN), x=1))
        self.assertFalse(checker.puts_onto_battlefield(whir, card(LANTERN), x=0))
        # An X nobody chose is not a cost.
        with self.assertRaises(ValueError):
            checker.parse_cost(whir.mana_cost)

    def test_whir_at_x_one(self):
        f = self.PLAY
        self.assertEqual(share(WHIR, self.LINE, lambda p: p.cast_by(WHIR, 3), 4, **f), 0)
        self.assertEqual(share(WHIR, self.LINE, lambda p: p.cast_by(WHIR, 4), 4, **f), Fraction(101, 132))
        self.assertEqual(
            share(WHIR, self.LINE, lambda p: p.on_battlefield_by(LANTERN, 4), 4, **f),
            Fraction(130, 132),
        )
        self.assertEqual(share(WHIR, self.LINE, lambda p: p.cast_by(LANTERN, 4), 4, **f), Fraction(10, 12))
        self.assertEqual(share(WHIR, self.LINE, lambda p: p.in_library(LANTERN, 4), 4, **f), Fraction(2, 132))

    def test_three_islands_and_seven_forests(self):
        lands = ["Island"] * 3 + ["Forest"] * 7
        self.assertEqual(
            share(WHIR, self.LINE, lambda p: p.cast_by(WHIR, 4), 4, lands=lands, **self.PLAY),
            Fraction(259, 660),
        )


if __name__ == "__main__":
    unittest.main()
