"""Splitting a seeded stream across processes changes no game.

compare.py deals each stream in chunks (checker.stream_chunks), one per
worker task, and adds the counts up. This holds those sums to what one
`checker.play` call over the whole stream counts, on the real decks.

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


class Chunks(unittest.TestCase):
    def test_chunked_stream_counts_what_one_stream_counts(self):
        for deck in ("lantern", "loam"):
            path = DECKS / f"{deck}.txt"
            library = checker.load_library(path, INDEX)
            cmdrs = checker.commander_cards(path, INDEX)
            questions = [q for q in checker.QUESTIONS if q.deck == deck and not q.pending]
            for draw in (False, True):
                with self.subTest(deck=deck, draw=draw):
                    seed, games = f"test:{deck}:{draw}", 250
                    whole = checker.play(library, questions, draw, games, seed, cmdrs)
                    summed = dict.fromkeys(whole, 0)
                    depth = checker.deal_depth(questions)
                    # 250 in runs of 60: four full runs and a short one.
                    for state, n in checker.stream_chunks(seed, len(library), depth, games, 60):
                        part = checker.play(library, questions, draw, n, seed, cmdrs, state)
                        for name, h in part.items():
                            summed[name] += h
                    self.assertEqual(summed, whole)


if __name__ == "__main__":
    unittest.main()
