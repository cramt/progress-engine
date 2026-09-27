"""What Rashmi and Ragavan's Treasure is worth to the Lantern north star (#81).
Runs against rashmi-treasure-checker.py beside it: the checker as it stood
when this was measured, with the Treasure's measuring parameters.

The north star: Lantern of Insight on the battlefield AND Rashmi and Ragavan
cast, by the end of turn 5, out of one mana budget. This deals shuffles of
`decks/lantern.deck.toml` and plays each deal twice through the independent
checker's line model (`checker/checker.py`, `line_path`), once with her
Treasure and once without, so the difference is paired, deal by deal.

The line is the checker's, with every Lantern route it models: the Lantern
hard cast, Trinket Mage fetching it to hand, Tezzeret the Seeker's -1 putting
it onto the battlefield, and Urza's Saga's chapter III two turns after the
Saga is played (played the turn it is first held). The rocks are ADR 0018's.
Two orders of the line are played:

  lantern-first   Lantern, Rashmi, the rocks, Trinket Mage, the Seeker
  rashmi-first    Rashmi, Lantern, the rocks, Trinket Mage, the Seeker
  +fillers        lantern-first, then any one-mana permanent (FILLERS)

What the Treasure is, and why it pays only for a spell after the turn's first,
is in the checker's line notes and the tests that shipped with the measurement. The free cast off
the opponent's exiled card is not counted.

Each deal is played three ways: without the Treasure, with it, and against a
ceiling in which every turn after Rashmi's opens with a free spell outside the
line, so the turn's Treasure pays even the line's first cast. The ceiling is
what the Treasure could be worth to a pilot with a spare cheap spell every
turn, and bounds what any richer line could get from it.

    python3 docs/research/rashmi-treasure.py [--games N] [--seed S] [--jobs J]

Standard library only. Reads nothing in crates/.
"""

from __future__ import annotations

import argparse
import math
import multiprocessing
import random
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
import importlib.util  # noqa: E402

_spec = importlib.util.spec_from_file_location(
    "checker", Path(__file__).with_name("rashmi-treasure-checker.py")
)
checker = importlib.util.module_from_spec(_spec)
sys.modules["checker"] = checker
_spec.loader.exec_module(checker)

RASHMI, LANTERN = checker.RASHMI, checker.LANTERN
ROCKS = tuple(entry for entry in checker.LANTERN_ROCK_LINE if entry != (RASHMI,))
TUTORS = ((checker.TRINKET,), (checker.TEZZERET,))
LINES = {
    "lantern-first": ((LANTERN,), (RASHMI,)) + ROCKS + TUTORS,
    "rashmi-first": ((RASHMI,), (LANTERN,)) + ROCKS + TUTORS,
}
# The deck's one-mana permanents that no route names, cast last: a turn whose
# line cast nothing else spends a spare mana on one, and that first spell
# banks a Treasure for a later turn. Instants are left out (they want targets).
FILLERS = (
    "Codex Shredder",
    "Dakra Mystic",
    "Expedition Map",
    "Field of Dreams",
    "Ghost Vacuum",
    "Ghoulcaller's Bell",
    "Pyxis of Pandemonium",
    "Relic of Progenitus",
    "Sensei's Divining Top",
    "Soldier of Fortune",
)
LINES["lantern-first+fillers"] = LINES["lantern-first"] + (FILLERS,)
SAGA = (("Urza's Saga", 2, LANTERN),)
TURNS = (5, 6)
# Seven, six draws on the draw, and one more for each card a route takes out.
DEPTH = 7 + max(TURNS) + 3

_library = _commanders = None


def _init() -> None:
    global _library, _commanders
    index = checker.Index(ROOT / "decks" / "index.jsonl")
    deck = ROOT / "decks" / "lantern.deck.toml"
    _library = checker.load_library(deck, index)
    _commanders = checker.commander_cards(deck, index)


def _asks(path: checker.LinePath) -> tuple[bool, ...]:
    out = []
    for t in TURNS:
        rashmi = path.cast_by(RASHMI, t)
        lantern = path.on_battlefield_by(LANTERN, t)
        out += [rashmi and lantern, rashmi]
    return tuple(out)


def _chunk(args: tuple[str, bool, int, int]) -> dict:
    seed, draw, start, n = args
    counts: dict = {}
    for i in range(start, start + n):
        rng = random.Random(f"{seed}:{draw}:{i}")
        cards = rng.sample(_library, DEPTH)
        for name, line in LINES.items():
            answers = []
            for treasure, ceiling in ((None, False), (RASHMI, False), (RASHMI, True)):
                g = checker.Game(
                    cards, draw, len(_library), commanders=_commanders, library=_library
                )
                p = checker.line_path(
                    g,
                    line,
                    max(TURNS),
                    checker.TRINKET_FETCHES,
                    checker.TEZZERET_PUTS,
                    treasure=treasure,
                    delayed=SAGA,
                    treasure_ceiling=ceiling,
                )
                answers.append(_asks(p))
            for q, (off, on, top) in enumerate(zip(*answers)):
                for kind, got in (("with", on), ("ceiling", top)):
                    # without, with, with-and-not-without
                    c = counts.setdefault((name, kind, q), [0, 0, 0])
                    c[0] += off
                    c[1] += got
                    c[2] += got and not off
    return counts


QUESTION = [f"{what} by turn {t}" for t in TURNS for what in ("north star", "Rashmi cast")]


def main() -> None:
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--games", type=int, default=200_000)
    ap.add_argument("--seed", default="81")
    ap.add_argument("--jobs", type=int, default=multiprocessing.cpu_count())
    a = ap.parse_args()
    step = 2_000
    with multiprocessing.Pool(a.jobs, initializer=_init) as pool:
        for draw in (False, True):
            chunks = [
                (a.seed, draw, s, min(step, a.games - s)) for s in range(0, a.games, step)
            ]
            total: dict = {}
            for part in pool.imap_unordered(_chunk, chunks):
                for k, v in part.items():
                    acc = total.setdefault(k, [0, 0, 0])
                    for j in range(3):
                        acc[j] += v[j]
            seat = "draw" if draw else "play"
            n = a.games
            for name, kind in ((n, k) for n in LINES for k in ("with", "ceiling")):
                for q, label in enumerate(QUESTION):
                    off, on, gained = total[(name, kind, q)]
                    lost = gained - (on - off)  # games the Treasure line lost
                    d = (on - off) / n
                    # Paired difference: the variance of a per-deal +1/0/-1.
                    var = ((gained + lost) / n - d * d) / n
                    half = 1.96 * math.sqrt(max(var, 0.0))
                    print(
                        f"{seat}  {name:21}  {label:22}  without {off / n:7.3%}  "
                        f"{kind:7} {on / n:7.3%}  delta {d * 100:+.3f}pp +/- {half * 100:.3f}"
                        f"  (gained {gained}, lost {lost})"
                    )


if __name__ == "__main__":
    main()
