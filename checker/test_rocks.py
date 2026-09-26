"""HANDS.md hands 26 to 33, held against the checker's rocks-and-dorks line.

Each hand is a seven-card library on the play, so the opening hand is the
whole deck and every answer is a yes or a no. "Filler" is a card the line
does not name.

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
FILLER = "Beast Within"


def card(name: str) -> checker.Card:
    return checker._make_card(INDEX.card(name), ())


def hand(*names: str, commander: str | None = None) -> checker.Game:
    cards = [card(n) for n in names]
    cards += [card(FILLER)] * (7 - len(cards))
    commanders = (card(commander),) if commander else ()
    return checker.Game(cards, False, len(cards), commanders=commanders)


def line(*names: str) -> checker.Line:
    return tuple((n,) for n in names)


def cast_on(game, ln, name, turn) -> bool:
    return checker.line_holds(game, ln, turn, lambda p: p[turn - 1].casts(name))


def cast_by(game, ln, name, turn) -> bool:
    first = checker.earliest_cast(game, ln, name, turn)
    return first is not None


def left_pays(game, ln, cost, turn) -> bool:
    return checker.line_holds(game, ln, turn, lambda p: p[turn - 1].left_pays(cost))


class Sources(unittest.TestCase):
    """What the oracle text makes of each rock and dork in the two decks."""

    def test_amounts_and_colours(self):
        identity = frozenset("GUR")
        expect = {
            "Sol Ring": (2, {"C"}, False),
            "Mind Stone": (1, {"C"}, False),
            "Arcane Signet": (1, {"G", "U", "R"}, False),
            "Talisman of Creativity": (1, {"C", "U", "R"}, False),
            "Talisman of Curiosity": (1, {"C", "G", "U"}, False),
            "Talisman of Impulse": (1, {"C", "R", "G"}, False),
            "Birds of Paradise": (1, set("WUBRG"), True),
            "Elvish Mystic": (1, {"G"}, True),
        }
        for name, (amount, palette, sick) in expect.items():
            with self.subTest(name):
                src = checker.mana_source(card(name), identity)
                self.assertEqual((src.amount, set(src.palette), src.sick), (amount, palette, sick))

    def test_not_sources(self):
        for name in ("Fellwar Stone", "Lotus Cobra", "Mana Drain", "Lantern of Insight", "Island"):
            with self.subTest(name):
                self.assertIsNone(checker.mana_source(card(name), frozenset("GUR")))

    def test_the_run_names_what_it_counts_as_nothing(self):
        cards = [card(n) for n in ("Fellwar Stone", "Lotus Cobra", "Sol Ring", "Forest")]
        self.assertEqual(checker.unmodelled_sources(cards), ["Fellwar Stone", "Lotus Cobra"])


class Hand26(unittest.TestCase):
    """Island, Sol Ring, Lantern of Insight: Sol Ring taps the turn it lands."""

    g = hand("Island", "Sol Ring", "Lantern of Insight")
    ln = line("Sol Ring", "Lantern of Insight")
    rev = line("Lantern of Insight", "Sol Ring")

    def test_the_line(self):
        self.assertTrue(cast_on(self.g, self.ln, "Lantern of Insight", 1))
        self.assertTrue(cast_on(self.g, self.ln, "Sol Ring", 1))
        self.assertTrue(left_pays(self.g, self.ln, "{1}", 1))

    def test_the_line_reversed(self):
        self.assertTrue(cast_on(self.g, self.rev, "Lantern of Insight", 1))
        self.assertFalse(cast_on(self.g, self.rev, "Sol Ring", 1))
        self.assertFalse(left_pays(self.g, self.rev, "{1}", 1))

    def test_lands_only(self):
        self.assertFalse(self.g.can_cast(1, "{1}{1}"))


class Hand27(unittest.TestCase):
    """Island, Sol Ring, Memory Lapse: a rock never pays for itself."""

    g = hand("Island", "Sol Ring", "Memory Lapse")
    ln = line("Sol Ring", "Memory Lapse")

    def test_not_on_turn_1(self):
        self.assertTrue(cast_on(self.g, self.ln, "Sol Ring", 1))
        self.assertFalse(cast_on(self.g, self.ln, "Memory Lapse", 1))

    def test_on_turn_2(self):
        self.assertTrue(cast_on(self.g, self.ln, "Memory Lapse", 2))

    def test_one_joint_matching_would_have_said_yes(self):
        # The naive model: {1} + {1}{U} against Island + {C}{C}, all at once.
        units = [(0, frozenset("U")), (0, frozenset("C")), (0, frozenset("C"))]
        self.assertTrue(checker._settles(units, [(1, []), (1, ["U"])]))
        nested = [(0, frozenset("U")), (1, frozenset("C")), (1, frozenset("C"))]
        self.assertFalse(checker._settles(nested, [(1, []), (1, ["U"])]))


class Hand28(unittest.TestCase):
    """Mind Stone listed first, unaffordable, and cast once Sol Ring grows the pool."""

    g = hand("Island", "Sol Ring", "Mind Stone")
    ln = line("Mind Stone", "Sol Ring")

    def test_read_again_from_the_top(self):
        self.assertTrue(cast_on(self.g, self.ln, "Mind Stone", 1))
        self.assertTrue(left_pays(self.g, self.ln, "{1}", 1))


class Hands29And30(unittest.TestCase):
    """Rashmi off a Talisman on turn 3, and never off a Mind Stone."""

    ln = line("Talisman of Creativity", "Mind Stone", "Rashmi and Ragavan")

    def test_talisman(self):
        g = hand("Island", "Forest", "Forest", "Talisman of Creativity", commander="Rashmi and Ragavan")
        self.assertFalse(cast_on(g, self.ln, "Talisman of Creativity", 1))
        self.assertTrue(cast_on(g, self.ln, "Talisman of Creativity", 2))
        self.assertTrue(cast_on(g, self.ln, "Rashmi and Ragavan", 3))
        self.assertEqual(checker.earliest_cast(g, self.ln, "Rashmi and Ragavan", 5), 3)

    def test_mind_stone(self):
        g = hand("Island", "Forest", "Forest", "Mind Stone", commander="Rashmi and Ragavan")
        self.assertTrue(cast_on(g, self.ln, "Mind Stone", 2))
        self.assertFalse(cast_by(g, self.ln, "Rashmi and Ragavan", 5))


class Hand31(unittest.TestCase):
    """Elvish Mystic is summoning-sick, and buys a {G} beside the Loam on turn 2."""

    g = hand("Forest", "Forest", "Forest", "Elvish Mystic", "Life from the Loam")
    ln = line("Elvish Mystic", "Life from the Loam")

    def test_no_mana_the_turn_it_arrives(self):
        self.assertTrue(cast_on(self.g, self.ln, "Elvish Mystic", 1))
        self.assertFalse(left_pays(self.g, self.ln, "{G}", 1))

    def test_loam_by_turn_2(self):
        self.assertEqual(checker.earliest_cast(self.g, self.ln, "Life from the Loam", 3), 2)

    def test_the_mystic_is_the_spare_mana_on_turn_2(self):
        # Loam is {1}{G} and two Forests pay it, so lands alone cast it on
        # turn 2 as well (HANDS.md's table once said otherwise). What the
        # Mystic buys on turn 2 is the {G} left beside it.
        self.assertTrue(self.g.can_cast(2, "{1}{G}"))
        self.assertTrue(left_pays(self.g, self.ln, "{G}", 2))
        lands_only = (("Life from the Loam",),)
        self.assertFalse(left_pays(self.g, lands_only, "{G}", 2))


class Hand32(unittest.TestCase):
    """Lotus Cobra is cast and counted as making nothing."""

    g = hand("Forest", "Forest", "Island", "Mountain", "Lotus Cobra", commander="Borborygmos and Fblthp")
    ln = line("Lotus Cobra", "Borborygmos and Fblthp")

    def test_cobra_is_cast_and_borborygmos_never(self):
        self.assertTrue(cast_on(self.g, self.ln, "Lotus Cobra", 2))
        self.assertFalse(cast_by(self.g, self.ln, "Borborygmos and Fblthp", 5))
        self.assertIn("Lotus Cobra", checker.unmodelled_sources(self.g.cards))


class Hand33(unittest.TestCase):
    """Fellwar Stone makes nothing with nobody else at the table; Mind Stone does."""

    ln = line("Fellwar Stone", "Mind Stone", "Trinket Mage")

    def test_fellwar(self):
        g = hand("Island", "Island", "Fellwar Stone", "Trinket Mage")
        self.assertTrue(cast_on(g, self.ln, "Fellwar Stone", 2))
        self.assertFalse(cast_by(g, self.ln, "Trinket Mage", 5))

    def test_mind_stone(self):
        g = hand("Island", "Island", "Mind Stone", "Trinket Mage")
        self.assertEqual(checker.earliest_cast(g, self.ln, "Trinket Mage", 5), 3)


if __name__ == "__main__":
    unittest.main()
