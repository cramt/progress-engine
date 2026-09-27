"""HANDS.md hands 58 and 59, held against the checker's line with its attack
and landfall mills.

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


def names(cards) -> list[str]:
    return [c.name for c in cards]


class Hand58(unittest.TestCase):
    """Six attacks the turn after it is cast."""

    def setUp(self):
        self.game = deal(
            "Forest", "Forest", "Forest", checker.SIX, BEAST, BEAST, BEAST,
            BEAST, BEAST, BEAST, checker.LOAM, "Mountain", BEAST, "Island",
            BEAST, "Forest", BEAST,
        )  # fmt: skip
        self.line = ((checker.SIX,),)

    def path(self, attacks):
        return checker.line_path(self.game, self.line, 5, attacks=attacks)

    def test_it_mills_from_the_turn_after_and_keeps_a_land(self):
        path = self.path(checker.LOAM_ATTACKS)
        self.assertTrue(path[2].casts(checker.SIX), "cast on turn 3")
        self.assertEqual(path[2].milled, [], "summoning-sick on turn 3")
        self.assertEqual(names(path[3].milled), [checker.LOAM, BEAST], "the Mountain is kept")
        self.assertEqual(names(path[4].milled), [BEAST, BEAST], "and on turn 5 the Forest")
        self.assertFalse(path.in_graveyard_by(checker.LOAM, 3))
        self.assertTrue(path.in_graveyard_by(checker.LOAM, 4))
        after = path[-1].game
        self.assertEqual(after.lands_played(4), 3, "the Mountain came after turn 4's drop")
        self.assertEqual(after.lands_played(5), 4)

    def test_without_the_attack_the_loam_is_drawn_on_turn_5(self):
        path = self.path({})
        self.assertFalse(path.in_graveyard_by(checker.LOAM, 5))
        self.assertIn(checker.LOAM, names(path[-1].game.seen(5)))


class Hand59(unittest.TestCase):
    """Icetill Explorer mills one for each land that enters after it."""

    def test_the_drop_of_the_turn_it_is_cast_came_before_it(self):
        game = deal(
            "Forest", "Forest", "Forest", "Forest", checker.EXPLORER, BEAST,
            BEAST, BEAST, BEAST, "Mountain", BEAST, checker.LOAM, BEAST,
        )  # fmt: skip
        path = checker.line_path(
            game, ((checker.EXPLORER,),), 6, landfalls=checker.LOAM_LANDFALLS
        )
        self.assertTrue(path[3].casts(checker.EXPLORER), "cast on turn 4")
        self.assertEqual(path[3].milled, [])
        self.assertEqual(names(path[4].milled), [checker.LOAM], "turn 5's Mountain")
        self.assertEqual(path[5].milled, [], "no land on turn 6")
        self.assertNotIn(checker.LOAM, names(path[-1].game.seen(6)))


class NorthStarLine(unittest.TestCase):
    def test_lumra_is_never_cast_by_turn_5(self):
        """It costs six and this deal holds no dork: six drops, turn 6."""
        game = deal(
            "Forest", "Forest", "Forest", "Forest", "Forest", "Forest",
            checker.LUMRA, "Forest", "Forest", "Forest", "Forest", "Forest",
        )  # fmt: skip
        path = checker._north_star_path(game)
        self.assertFalse(path.cast_by(checker.LUMRA, 5))
        self.assertTrue(
            checker.line_path(game, ((checker.LUMRA,),), 6).cast_by(checker.LUMRA, 6)
        )


if __name__ == "__main__":
    unittest.main()
