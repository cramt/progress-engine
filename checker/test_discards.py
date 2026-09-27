"""HANDS.md hands 21 to 24, held against the checker's declared line.

Each hand is dealt in the order the hand writes its library down, on the play,
so every answer is a yes or a no. Where the checker's draw steps need cards the
hand does not name, they are Beast Within, which nothing here casts.

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
BORBORYGMOS = checker.BORBORYGMOS


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def deal(*names: str, commander: str | None = None) -> checker.Game:
    """The opening seven and then the library, top first, in one list."""
    cards = [card(n) for n in names]
    commanders = (card(commander),) if commander else ()
    return checker.Game(cards, False, len(cards), library=cards, commanders=commanders)


def is_beast(c: checker.Card) -> bool:
    return c.name == BEAST


def play(game, line, turns, discard, rummages):
    return checker.declared_line_path(
        game, line, turns, (checker._is_land,), discard, {}, {}, rummages
    )


FRANTIC = {checker.FRANTIC: checker.LOAM_RUMMAGES[checker.FRANTIC]}


class Hand21(unittest.TestCase):
    """Frantic Search, and the discard list decides where the Loam goes."""

    def setUp(self):
        self.game = deal(
            "Island", "Island", "Island", checker.FRANTIC, BEAST, BEAST, BEAST,
            BEAST, BEAST, checker.LOAM, "Forest",
        )  # fmt: skip

    def test_a_list_naming_the_loam_bins_it_and_the_forest_it_drew(self):
        path = play(self.game, ((checker.FRANTIC,),), 3, (checker._is_loam, checker._is_land), FRANTIC)
        self.assertTrue(path.cast_by(checker.FRANTIC, 3))
        self.assertTrue(path.in_graveyard_by(checker.LOAM, 3))
        self.assertTrue(path.in_graveyard_by("Forest", 3))
        self.assertFalse(path.in_graveyard_by(BEAST, 3))

    def test_a_list_naming_beast_within_keeps_the_loam(self):
        path = play(self.game, ((checker.FRANTIC,),), 3, (is_beast,), FRANTIC)
        self.assertFalse(path.in_graveyard_by(checker.LOAM, 3))
        gone = [c.name for c in path[2].to_graveyard]
        self.assertEqual(sorted(gone), [BEAST, BEAST, checker.FRANTIC])


class Hand22(unittest.TestCase):
    """Desperate Ravings discards at random, whatever the list says."""

    def setUp(self):
        self.game = deal(
            "Mountain", "Mountain", checker.RAVINGS, BEAST, BEAST, BEAST, BEAST,
            BEAST, checker.LOAM, "Forest",
        )  # fmt: skip
        self.ravings = {checker.RAVINGS: checker.LOAM_RUMMAGES[checker.RAVINGS]}

    def test_one_card_goes_and_the_list_does_not_choose_it(self):
        listed = play(self.game, ((checker.RAVINGS,),), 2, (checker._is_loam,), self.ravings)
        bare = play(self.game, ((checker.RAVINGS,),), 2, (), self.ravings)
        self.assertTrue(listed.cast_by(checker.RAVINGS, 2))
        gone = [c.name for c in listed[1].to_graveyard if c.name != checker.RAVINGS]
        self.assertEqual(len(gone), 1, "one card, at random")
        self.assertEqual(
            gone, [c.name for c in bare[1].to_graveyard if c.name != checker.RAVINGS]
        )

    def test_over_many_deals_the_loam_goes_one_time_in_seven(self):
        # The same hand, with the library's filler named differently so that
        # each deal picks afresh: the Loam is one card of seven in hand.
        hits, deals = 0, 2000
        for i in range(deals):
            game = deal(
                "Mountain", "Mountain", checker.RAVINGS, BEAST, BEAST, BEAST, BEAST,
                BEAST, checker.LOAM, "Forest", *(["Island"] * (i % 7)), *(["Forest"] * (i // 7)),
            )  # fmt: skip
            path = play(game, ((checker.RAVINGS,),), 2, (checker._is_loam,), self.ravings)
            hits += path.in_graveyard_by(checker.LOAM, 2)
        # 1/7 of 2000 is 286; four standard errors either side is about 60.
        self.assertLess(abs(hits - deals / 7), 60, hits)


class Hand23(unittest.TestCase):
    """Borborygmos and Fblthp cannot discard the Loam."""

    def setUp(self):
        self.game = deal(
            "Island", "Island", "Forest", "Forest", "Mountain", "Mountain", checker.LOAM,
            BEAST, BEAST, BEAST, BEAST, "Forest",
            commander=BORBORYGMOS,
        )  # fmt: skip
        self.enters = {
            BORBORYGMOS: checker.Rummage(draw=1, any_number=True, only=checker._is_land)
        }

    def test_any_number_is_every_land_the_list_names_and_never_the_loam(self):
        path = play(
            self.game, ((BORBORYGMOS,),), 5, (checker._is_loam, checker._is_land), self.enters
        )
        self.assertTrue(path.cast_by(BORBORYGMOS, 5))
        self.assertFalse(path.in_graveyard_by(checker.LOAM, 5))
        lands = [c.name for c in path[4].to_graveyard if c.playable_land]
        self.assertEqual(sorted(lands), ["Forest", "Mountain"])

    def test_a_list_naming_only_the_loam_discards_nothing(self):
        path = play(self.game, ((BORBORYGMOS,),), 5, (checker._is_loam,), self.enters)
        self.assertTrue(path.cast_by(BORBORYGMOS, 5))
        self.assertEqual(path[4].to_graveyard, [])


class Hand24(unittest.TestCase):
    """A spell drawn mid-line is cast; a land drawn mid-line waits."""

    def setUp(self):
        self.game = deal(
            "Forest", "Island", checker.FRANTIC, BEAST, BEAST, BEAST, BEAST,
            "Island", "Island", checker.LOAM, "Forest",
        )  # fmt: skip
        self.line = ((checker.FRANTIC,), (checker.LOAM,))

    def test_the_untap_pays_for_the_loam_it_drew(self):
        path = play(self.game, self.line, 3, (is_beast,), FRANTIC)
        self.assertTrue(path.cast_by(checker.FRANTIC, 3))
        self.assertTrue(path.cast_by(checker.LOAM, 3))
        self.assertEqual(path[2].lands_in_play, 3, "the Forest it drew waits")

    def test_a_list_that_bins_the_loam_casts_nothing_more(self):
        path = play(self.game, self.line, 3, (checker._is_loam, is_beast), FRANTIC)
        self.assertFalse(path.cast_by(checker.LOAM, 3))
        self.assertTrue(path.in_graveyard_by(checker.LOAM, 3))
        self.assertEqual(path[2].lands_in_play, 3)


if __name__ == "__main__":
    unittest.main()
