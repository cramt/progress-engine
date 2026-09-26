"""Hold the engine's exact answers against the independent checker's.

For every question in checker.QUESTIONS, on both seats, this runs
`gauntlet test decks/<deck>.txt decks/<criteria> --index decks/index.jsonl`,
reads the criterion's probability out of the JSON, deals the same number of
games through checker.py, and asks whether the engine's number lies inside the
checker's 99.9% interval. It exits 1 when any does not.

An answer the engine ESTIMATED rather than enumerated has an error bar of its
own, printed in the JSON as `standard_error`, so it is held to the interval of
the difference between two independent samples instead: both errors, added in
quadrature. That is a weaker check - it tests what a dealt game does, not the
enumeration - and the verdict says which kind it was.

A question marked `pending` (checker.Question.pending names the engine ticket)
is one the engine cannot answer yet. Its checker number is reported, on
--pending-games games, and fails nothing. Its criteria file need not exist;
if it does and the engine answers the criterion, the answer is compared like
any other and the verdict says to drop the marker. Flipping one to compared is
deleting its `pending=` argument.

    python3 checker/compare.py [--gauntlet PATH] [--games N] [--seed S]

The binary defaults to $GAUNTLET, then target/release/gauntlet.
"""

from __future__ import annotations

import argparse
import json
import math
import os
import subprocess
import sys
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import checker  # noqa: E402

Z_999 = 3.2905  # two-sided 99.9%


def engine_answers(gauntlet: str, decks: Path, deck: str, criteria: str, draw: bool) -> dict:
    cmd = [
        gauntlet,
        "test",
        str(decks / f"{deck}.txt"),
        str(decks / criteria),
        "--index",
        str(decks / "index.jsonl"),
    ]
    if draw:
        cmd.append("--draw")
    # A failed assertion exits non-zero and still prints the JSON; only a
    # missing JSON is our failure.
    run = subprocess.run(cmd, capture_output=True, text=True)
    try:
        out = json.loads(run.stdout)
    except json.JSONDecodeError:
        sys.stderr.write(run.stderr)
        raise SystemExit(f"compare: {' '.join(cmd)} printed no JSON")
    return {c["name"]: c for c in out["criteria"]}


def judge(q, e: dict, hits: int, games: int) -> tuple[tuple, int]:
    """One compared row - (name, engine, checker, half-width, verdict) - and 1
    if it is a disagreement."""
    p_engine = e["probability"]
    p_check = hits / games
    # The interval is the checker's sampling error under the hypothesis that
    # the engine is right, so a p of 0 or 1 is not an interval of zero width
    # around an estimate.
    variance = max(p_engine * (1 - p_engine), 1e-12) / games
    sampled = e["method"] != "exact"
    if sampled:
        # Two estimates, each with its own error: the interval is of their
        # difference.
        variance += (e.get("standard_error") or 0.0) ** 2
    half = Z_999 * math.sqrt(variance)
    if abs(p_check - p_engine) <= half:
        return (q.name, p_engine, p_check, half, "agree (engine sampled)" if sampled else "agree"), 0
    return (q.name, p_engine, p_check, half, "DISAGREE"), 1


def main() -> int:
    here = Path(__file__).resolve().parent
    p = argparse.ArgumentParser(description="checker vs engine")
    p.add_argument(
        "--gauntlet",
        default=os.environ.get("GAUNTLET", str(here.parent / "target/release/gauntlet")),
    )
    p.add_argument("--decks", type=Path, default=here.parent / "decks")
    # 400,000 games puts the 99.9% half-width at 0.26pp at worst (p = 0.5).
    p.add_argument("--games", type=int, default=400_000)
    # The pending questions are only reported; 20,000 games keeps CI near three minutes;
    # pass --pending-games 100000 for a 0.52pp half-width.
    p.add_argument("--pending-games", type=int, default=20_000)
    p.add_argument("--seed", default="0")
    args = p.parse_args()

    index = checker.Index(args.decks / "index.jsonl")
    checker.check_commanders(args.decks, index)
    started = time.monotonic()
    rows, failures = [], 0
    for deck in sorted({q.deck for q in checker.QUESTIONS}):
        library = checker.load_library(args.decks / f"{deck}.txt", index)
        cmdrs = checker.commander_cards(args.decks / f"{deck}.txt", index)
        compared = [q for q in checker.QUESTIONS if q.deck == deck and not q.pending]
        pending = [q for q in checker.QUESTIONS if q.deck == deck and q.pending]
        for draw in (False, True):
            seat = "draw" if draw else "play"
            engine = {}
            for criteria in sorted({q.criteria for q in compared + pending}):
                if any(q.criteria == criteria for q in compared) or (args.decks / criteria).exists():
                    engine.update(engine_answers(args.gauntlet, args.decks, deck, criteria, draw))
            seed = f"{args.seed}:{deck}:{seat}"
            hits = checker.play(library, compared, draw, args.games, seed, cmdrs)
            for q in compared:
                if q.name not in engine:
                    raise SystemExit(f"compare: the engine answered no criterion named {q.name!r}")
                row, failed = judge(q, engine[q.name], hits[q.name], args.games)
                rows.append((deck, seat) + row)
                failures += failed
            if not pending:
                continue
            games = args.pending_games
            hits = checker.play(library, pending, draw, games, seed + ":pending", cmdrs)
            for q in pending:
                if q.name in engine:
                    row, failed = judge(q, engine[q.name], hits[q.name], games)
                    row = row[:-1] + (f"{row[-1]}; the engine answers it, drop pending",)
                    failures += failed
                else:
                    p_check = hits[q.name] / games
                    half = Z_999 * math.sqrt(max(p_check * (1 - p_check), 1e-12) / games)
                    row = (q.name, None, p_check, half, f"pending engine ({q.pending})")
                rows.append((deck, seat) + row)

    width = max(len(r[2]) for r in rows)
    print(f"{'deck':8} {'seat':4}  {'question':{width}}  {'engine':>9}  {'checker':>9}  {'99.9% ±':>8}  verdict")
    for deck, seat, name, pe, pc, half, verdict in rows:
        engine_cell = f"{pe:9.4%}" if pe is not None else f"{'-':>9}"
        print(f"{deck:8} {seat:4}  {name:{width}}  {engine_cell}  {pc:9.4%}  {half:8.4%}  {verdict}")
    elapsed = time.monotonic() - started
    print(
        f"\n{args.games:,} games per deck and seat ({args.pending_games:,} for pending"
        f" questions), seed {args.seed!r}, {elapsed:.1f}s"
    )
    if failures:
        print(f"FAIL: {failures} answer(s) outside the 99.9% interval")
        return 1
    print("OK: every answer is inside its 99.9% interval")
    return 0


if __name__ == "__main__":
    sys.exit(main())
