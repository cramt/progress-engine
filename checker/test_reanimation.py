"""A cast returns cards from the graveyard to the battlefield (#140), held
against the checker's declared line: Animate Dead waits for a card to return,
returns the one its list names first, and what comes back was never cast.

Written from README "Reanimation" and the cards' text. Each hand is dealt in
the order it is written down, on the play, so every answer is a yes or a no.

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
ANIMATE = "Animate Dead"
CID = "Cid, Timeless Artificer"


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def deal(*names: str) -> checker.Game:
    cards = [card(n) for n in names]
    return checker.Game(cards, False, len(cards), library=cards)


def play(game, line, turns, returns):
    return checker.declared_line_path(
        game, line, turns, (checker._is_land,), (), {}, {checker.ANALYST: checker.Mill(3)}, {},
        returns=returns,
    )  # fmt: skip


def creature(c: checker.Card) -> bool:
    return "Creature" in c.type_line


def named(name: str):
    return lambda c: c.name == name


class AnimateDeadWaitsForTheGraveyard(unittest.TestCase):
    def setUp(self):
        # Forest, Swamp, Swamp, Aftermath Analyst and Animate Dead in hand.
        # Turn 2: Animate Dead is first in the line and the graveyard is
        # empty, so the Analyst is cast and mills Cid, Beast Within and a
        # second Aftermath Analyst under the turn's draw; the two mana are
        # spent. Turn 3: Animate
        # Dead returns the creature its list names first.
        self.game = deal(
            "Forest", "Swamp", "Swamp", checker.ANALYST, ANIMATE, BEAST, BEAST,
            BEAST, CID, BEAST, checker.ANALYST, BEAST, BEAST,
        )  # fmt: skip
        self.line = ((ANIMATE,), (checker.ANALYST,))

    def path(self, *prefer):
        returns = {ANIMATE: checker.Reanimation(creature, 1, prefer)}
        return play(self.game, self.line, 3, returns)

    def test_it_returns_the_card_its_list_names_first(self):
        path = self.path(named(CID), creature)
        self.assertTrue(path.cast_by(checker.ANALYST, 2))
        self.assertFalse(path.cast_by(ANIMATE, 2), "held, then out of mana")
        self.assertTrue(path.cast_by(ANIMATE, 3))
        self.assertEqual(path.reanimated_by(CID, 3), 1)
        self.assertEqual(path.reanimated_by(checker.ANALYST, 3), 0)
        self.assertFalse(path.cast_by(CID, 3), "it came back, it was not cast")
        path = self.path(named(checker.ANALYST), creature)
        self.assertEqual(path.reanimated_by(checker.ANALYST, 3), 1)
        self.assertEqual(path.reanimated_by(CID, 3), 0)

    def test_it_waits_for_a_card_its_list_names(self):
        # Nothing the list names is ever in the graveyard: it is never cast.
        path = self.path(named("Birds of Paradise"))
        self.assertFalse(path.cast_by(ANIMATE, 3))

    def test_every_one_comes_back_where_the_card_returns_all(self):
        returns = {ANIMATE: checker.Reanimation(creature)}
        path = play(self.game, self.line, 3, returns)
        self.assertEqual(path.reanimated_by(CID, 3), 1)
        self.assertEqual(path.reanimated_by(checker.ANALYST, 3), 1)


if __name__ == "__main__":
    unittest.main()
