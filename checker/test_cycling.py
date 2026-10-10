"""A card the line cycles rather than casts (#136), held against the checker's
declared line: Cid, Timeless Artificer pays {W}{U} from hand, goes to the
graveyard and draws a card, and is never cast; a Cid it draws is cycled the
same turn when the pool still pays.

Written from README "Cycling", HANDS.md hand 65 and Cid's text. Each hand is
dealt in the order it is written down, on the play, so every answer is a yes
or a no.

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
CID = "Cid, Timeless Artificer"
BLANK = "Ornithopter"
# "Cycling {W}{U} ({W}{U}, Discard this card: Draw a card.)"
CYCLES = {CID: checker.Cycle("{W}{U}", 1)}
LINE = ((CID,),)


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def deal(*names: str) -> checker.Game:
    cards = [card(n) for n in names]
    return checker.Game(cards, False, len(cards), library=cards)


def islands_first(c: checker.Card) -> bool:
    return c.name == "Island"


def play(game, turns, cycles=CYCLES):
    return checker.declared_line_path(
        game, LINE, turns, (islands_first, checker._is_land), (), {}, {}, {},
        cycles=cycles,
    )  # fmt: skip


def cids_in_graveyard(path, turn: int) -> int:
    return sum(c.name == CID for t in path[:turn] for c in t.to_graveyard)


class CidCyclesIntoTheGraveyard(unittest.TestCase):
    def test_two_lands_of_both_colours_cycle_one_and_cast_none(self):
        # Island on turn 1, Plains on turn 2: one {W}{U}, one cycle, and the
        # card it draws is the second Cid, which waits for turn 3.
        game = deal(
            "Island", "Plains", CID, BLANK, "Plains", BLANK, BLANK,
            BLANK, CID, BLANK, BLANK,
        )  # fmt: skip
        path = play(game, 3)
        self.assertEqual(cids_in_graveyard(path, 1), 0, "one land pays no {W}{U}")
        self.assertEqual(cids_in_graveyard(path, 2), 1)
        self.assertEqual(cids_in_graveyard(path, 3), 2)
        self.assertFalse(path.cast_by(CID, 3), "a cycled Cid is never cast")

    def test_one_colour_cycles_nothing(self):
        game = deal("Plains", "Plains", CID, CID, "Plains", BLANK, BLANK, BLANK, BLANK)
        path = play(game, 3)
        self.assertEqual(cids_in_graveyard(path, 3), 0)

    def test_a_cid_a_cycle_draws_is_cycled_the_same_turn(self):
        # Islands on turns 1 and 2, Plains on 3 and 4. Turn 3 pays one cycle,
        # which draws a Cid that waits; turn 4 pays two: that Cid, and the
        # one it draws.
        game = deal(
            "Island", "Island", "Plains", "Plains", CID, BLANK, BLANK,
            BLANK, BLANK, CID, BLANK, CID, BLANK, BLANK,
        )  # fmt: skip
        path = play(game, 4)
        self.assertEqual(cids_in_graveyard(path, 2), 0)
        self.assertEqual(cids_in_graveyard(path, 3), 1)
        self.assertEqual(cids_in_graveyard(path, 4), 3)
        self.assertFalse(path.cast_by(CID, 4))

    def test_without_the_cycle_the_line_casts_cid_at_four(self):
        # The same deal with Cid read as the creature it is: {2}{W}{U} on
        # turn 4, and nothing in the graveyard.
        game = deal(
            "Island", "Island", "Plains", "Plains", CID, BLANK, BLANK,
            BLANK, BLANK, CID, BLANK, CID, BLANK, BLANK,
        )  # fmt: skip
        path = play(game, 4, cycles={})
        self.assertEqual(cids_in_graveyard(path, 4), 0)
        self.assertTrue(path.cast_by(CID, 4))


if __name__ == "__main__":
    unittest.main()
