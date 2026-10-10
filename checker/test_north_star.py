"""The pieces the Loam north star adds to the checker's declared line: a dork
the line casts pays from the turn after (ADR 0018, CR 302.6), the commander is
cast from the command zone out of the same pool, and Lumra's lands come back
from the graveyard, fire a landfall and pay for nothing (README "Reanimation").

Each hand is dealt in the order it is written down, on the play, so every
answer is a yes or a no.

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
BIRDS = "Birds of Paradise"


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def deal(*names: str, commander: str | None = None) -> checker.Game:
    cards = [card(n) for n in names]
    commanders = (card(commander),) if commander else ()
    return checker.Game(cards, False, len(cards), library=cards, commanders=commanders)


def play(game, line, turns, **kw):
    return checker.declared_line_path(
        game, line, turns, (checker._is_land,), (checker._is_loam,), {}, kw.pop("mills", {}), {}, **kw
    )


class ADorkPaysFromTheNextTurn(unittest.TestCase):
    def setUp(self):
        # Four lands of the three colours by turn 4, and Birds on turn 1.
        self.game = deal(
            "Forest", "Island", "Mountain", "Forest", BIRDS, BEAST, BEAST,
            BEAST, BEAST, BEAST, BEAST, BEAST,
            commander=checker.BORBORYGMOS,
        )  # fmt: skip

    def test_birds_makes_the_commander_a_turn_early(self):
        path = play(self.game, ((checker.BORBORYGMOS,), (BIRDS,)), 5)
        self.assertTrue(path.cast_by(BIRDS, 1))
        self.assertFalse(path.cast_by(checker.BORBORYGMOS, 3), "four mana on turn 3")
        self.assertTrue(path.cast_by(checker.BORBORYGMOS, 4), "four lands and the Birds")

    def test_without_the_birds_in_the_line_it_waits_for_the_fifth_land(self):
        path = play(self.game, ((checker.BORBORYGMOS,),), 5)
        self.assertFalse(path.cast_by(checker.BORBORYGMOS, 5), "four lands only, ever")

    def test_a_dork_cast_this_turn_does_not_pay_this_turn(self):
        # Birds first on turn 1 off the Forest; nothing else costs {G} or less.
        game = deal("Forest", BIRDS, checker.LOAM, BEAST, BEAST, BEAST, BEAST, BEAST, BEAST)
        path = play(game, ((BIRDS,), (checker.LOAM,)), 2)
        self.assertTrue(path.cast_by(BIRDS, 1))
        self.assertFalse(path.cast_by(checker.LOAM, 1), "the Birds is summoning-sick")
        self.assertTrue(path.cast_by(checker.LOAM, 2), "one Forest and the Birds")


class LumraReturnsTheLands(unittest.TestCase):
    def setUp(self):
        # Five Forests and an Island by turn 6, the Explorer cast on turn 4
        # and Lumra on turn 6. The Explorer mills the Mountain off turn 5's
        # drop and the Loam off turn 6's; Lumra mills four, a Forest and an
        # Island among them, and returns every land in the yard, whichever
        # turn put it there: three lands, and the Explorer mills one for each.
        self.game = deal(
            "Forest", "Forest", "Forest", "Forest", "Forest", checker.EXPLORER, checker.LUMRA,
            "Island", BEAST, BEAST, BEAST, "Mountain", BEAST, checker.LOAM,
            "Forest", BEAST, "Island", BEAST, BEAST, BEAST, BEAST, BEAST, BEAST,
        )  # fmt: skip
        self.kw = dict(
            mills={checker.LUMRA: checker.Mill(4)},
            landfalls=checker.LOAM_LANDFALLS,
            returns={checker.LUMRA: checker.Reanimation(checker._is_land)},
        )

    def test_the_lands_come_back_and_fire_the_explorer(self):
        path = play(self.game, ((checker.LUMRA,), (checker.EXPLORER,)), 6, **self.kw)
        self.assertTrue(path.cast_by(checker.EXPLORER, 4))
        self.assertTrue(path.cast_by(checker.LUMRA, 6))
        self.assertEqual([c.name for c in path[4].to_graveyard], ["Mountain"])
        self.assertEqual(
            [c.name for c in path[5].to_graveyard],
            [checker.LOAM, "Forest", BEAST, "Island", BEAST, BEAST, BEAST, BEAST],
        )
        self.assertTrue(path.in_graveyard_by(checker.LOAM, 6))
        self.assertEqual(path[5].lands_in_play, 9, "six drops and three returned lands")

    def test_a_returned_land_pays_for_nothing(self):
        # The returned Mountain is the deck's only red: counted, it would pay
        # for the commander on turn 7 beside the drops' five Forests and an
        # Island. It took no drop, so it does not.
        game = deal(*[c.name for c in self.game.cards], commander=checker.BORBORYGMOS)
        line = ((checker.LUMRA,), (checker.EXPLORER,), (checker.BORBORYGMOS,))
        path = play(game, line, 7, **self.kw)
        self.assertTrue(path.cast_by(checker.LUMRA, 6))
        self.assertFalse(path.cast_by(checker.BORBORYGMOS, 7))


if __name__ == "__main__":
    unittest.main()
