"""HANDS.md hands 19 and 20, held against the checker's line with its mills.

Each hand is dealt in the order the hand writes its library down, on the play,
so every answer is a yes or a no.

    python3 -m unittest discover -s checker
"""

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import checker  # noqa: E402

DECKS = Path(os.environ.get("CHECKER_DECKS", HERE.parent / "decks"))
INDEX = checker.Index(DECKS / "index.jsonl")
BEAST = "Beast Within"


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def deal(*names: str) -> checker.Game:
    """The opening seven and then the library, top first, in one list."""
    cards = [card(n) for n in names]
    return checker.Game(cards, False, len(cards), library=cards)


class Hand19(unittest.TestCase):
    """Aftermath Analyst mills the Loam before you could draw it."""

    def setUp(self):
        self.game = deal(
            "Forest", "Forest", checker.ANALYST, BEAST, BEAST, BEAST, BEAST,
            BEAST, "Mountain", checker.LOAM, "Island", "Forest",
        )  # fmt: skip
        self.line = ((checker.ANALYST,),)

    def test_the_mill_takes_the_three_under_the_turn_two_draw(self):
        path = checker.line_path(self.game, self.line, 4, mills=checker.LOAM_MILLS)
        self.assertTrue(path[1].casts(checker.ANALYST), "cast on turn 2")
        self.assertEqual([c.name for c in path[1].milled], ["Mountain", checker.LOAM, "Island"])
        self.assertTrue(path.in_graveyard_by(checker.LOAM, 2))
        # Turn 3 draws the Forest that was under the three, and plays it.
        after = path[-1].game
        self.assertEqual(after.lands_played(3), 3)
        self.assertNotIn(checker.LOAM, [c.name for c in after.seen(4)])

    def test_without_the_mill_the_loam_is_drawn_on_turn_4(self):
        path = checker.line_path(self.game, self.line, 4)
        self.assertTrue(path[1].casts(checker.ANALYST), "casting it does not depend on the mill")
        self.assertFalse(path.in_graveyard_by(checker.LOAM, 4))
        self.assertIn(checker.LOAM, [c.name for c in path[-1].game.seen(4)])
        self.assertEqual(path[-1].game.lands_played(3), 3, "the Mountain, drawn on turn 3")


class Hand20(unittest.TestCase):
    """Malevolent Rumble keeps a permanent, and the Loam is not one."""

    def rumble(self, *prefer):
        game = deal(
            "Forest", "Forest", checker.RUMBLE, BEAST, BEAST, BEAST, BEAST,
            BEAST, checker.LOAM, "Mountain", BEAST, checker.ANALYST, BEAST,
        )  # fmt: skip
        mill = checker.Mill(4, keep_up_to=1, keep_only=checker._is_permanent_card, prefer=prefer)
        return checker.line_path(game, ((checker.RUMBLE,),), 3, mills={checker.RUMBLE: mill})

    def test_three_columns(self):
        loam = lambda c: c.name == checker.LOAM  # noqa: E731
        for prefer, milled, lands in (
            ((), 4, 2),
            ((checker._is_land,), 3, 3),
            ((loam, checker._is_land), 3, 3),
        ):
            path = self.rumble(*prefer)
            self.assertTrue(path[1].casts(checker.RUMBLE))
            self.assertTrue(path.in_graveyard_by(checker.LOAM, 2), prefer)
            self.assertEqual(len(path[1].milled), milled, prefer)
            self.assertEqual(path[-1].game.lands_played(3), lands, prefer)

    def test_a_kept_land_waits_for_the_next_drop(self):
        path = self.rumble(checker._is_land)
        self.assertEqual(path[-1].game.lands_played(2), 2, "turn 2's drop came before Rumble")


class WrennAndSeven(unittest.TestCase):
    def test_every_land_goes_to_hand_and_the_rest_to_the_graveyard(self):
        game = deal(
            "Forest", "Forest", "Forest", "Forest", "Forest", checker.WRENN, BEAST,
            BEAST, BEAST, BEAST, BEAST, checker.LOAM, "Mountain", BEAST, "Island",
        )  # fmt: skip
        path = checker.line_path(game, ((checker.WRENN,),), 5, mills=checker.LOAM_MILLS)
        self.assertTrue(path[4].casts(checker.WRENN), "five Forests by turn 5")
        self.assertEqual(
            sorted(c.name for c in path[4].milled), sorted([checker.LOAM, BEAST])
        )
        self.assertEqual(len(path[4].game.kept_lands), 2)


if __name__ == "__main__":
    unittest.main()
