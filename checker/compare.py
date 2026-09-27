"""Hold the engine's exact answers against the independent checker's.

For every question in checker.QUESTIONS, on both seats, this runs
`gauntlet test decks/<deck>.deck.toml decks/<criteria> --index decks/index.jsonl`,
reads the criterion's probability out of the JSON, deals the same number of
games through checker.py, and asks whether the engine's number lies inside the
checker's 99.9% interval. It exits 1 when any does not.

An answer the engine ESTIMATED rather than enumerated has an error bar of its
own, printed in the JSON as `standard_error`, so it is held to the interval of
the difference between two independent samples instead: both errors, added in
quadrature. That is a weaker check - it tests what a dealt game does, not the
enumeration - and the verdict says which kind it was.

A question may cap its own game count (checker.Question.games), where the
checker's model of it is slow and the engine samples it anyway; those are
dealt a run of their own, and held to the interval that many games give.

A question marked `pending` (checker.Question.pending names the engine ticket)
is one the engine cannot answer yet. Its checker number is reported, on
--pending-games games, and fails nothing. Its criteria file need not exist;
if it does and the engine answers the criterion, the answer is compared like
any other and the verdict says to drop the marker. Flipping one to compared is
deleting its `pending=` argument.

The games are dealt across --jobs worker processes (default: every CPU this
process may use) while the engine runs beside them. Each seeded stream is cut
into runs of CHUNK games at the generator's state where the run begins
(checker.stream_chunks), so the split deals the very games one process would
have: the numbers depend on --seed and never on --jobs.

    python3 checker/compare.py [--gauntlet PATH] [--games N] [--seed S] [--jobs J]

The binary defaults to $GAUNTLET, then target/release/gauntlet.
"""

from __future__ import annotations

import argparse
import concurrent.futures
import json
import math
import multiprocessing
import os
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import checker  # noqa: E402

Z_999 = 3.2905  # two-sided 99.9%


def engine_answers(gauntlet: str, decks: Path, deck: str, criteria: str, draw: bool) -> dict:
    cmd = [
        gauntlet,
        "test",
        str(decks / f"{deck}.deck.toml"),
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


# A run of this many games is one task for a worker process: small enough that
# four workers finish together, large enough that sending it costs nothing.
CHUNK = 5_000


@dataclass(frozen=True)
class Deal:
    """One seeded stream of games on one deck and seat, and the questions
    asked of it: what one `checker.play` call used to be."""

    deck: str
    draw: bool
    names: tuple[str, ...]
    games: int
    seed: str


# Each worker process holds the decks once (a forked one inherits them); a
# task then names its deal.
_decks: dict[str, tuple[list, tuple]] = {}


def _load_decks(decks: Path) -> None:
    if _decks:
        return
    index = checker.Index(decks / "index.jsonl")
    for deck in sorted({q.deck for q in checker.QUESTIONS}):
        path = decks / f"{deck}.deck.toml"
        _decks[deck] = (checker.load_library(path, index), checker.commander_cards(path, index))


def _questions(names: tuple[str, ...]) -> list:
    return [q for q in checker.QUESTIONS if q.name in names]


def _play_chunk(task: tuple[int, Deal, tuple, int]) -> tuple[int, dict[str, int]]:
    i, deal, state, n = task
    library, cmdrs = _decks[deal.deck]
    hits = checker.play(library, _questions(deal.names), deal.draw, n, deal.seed, cmdrs, state)
    return i, hits


def _chunks(deals: list[Deal]):
    for i, deal in enumerate(deals):
        size = len(_decks[deal.deck][0])
        depth = checker.deal_depth(_questions(deal.names))
        for state, n in checker.stream_chunks(deal.seed, size, depth, deal.games, CHUNK):
            yield i, deal, state, n


def deal_all(deals: list[Deal], decks: Path, jobs: int) -> list[dict[str, int]]:
    """Every deal's hits, dealt in chunks across `jobs` processes. The chunks
    are cut from each deal's one stream (checker.stream_chunks), so the counts
    are the ones a single process dealing them in turn would find."""
    totals: list[dict[str, int]] = [dict.fromkeys(d.names, 0) for d in deals]
    if jobs <= 1:
        done = map(_play_chunk, _chunks(deals))  # in this process, one after another
        return _add_up(totals, done)
    with multiprocessing.Pool(jobs, initializer=_load_decks, initargs=(decks,)) as pool:
        return _add_up(totals, pool.imap_unordered(_play_chunk, _chunks(deals)))


def _add_up(totals: list[dict[str, int]], done) -> list[dict[str, int]]:
    for i, hits in done:
        for name, h in hits.items():
            totals[i][name] += h
    return totals


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
    # The pending questions are only reported; 20,000 games keeps them cheap;
    # pass --pending-games 100000 for a 0.52pp half-width.
    p.add_argument("--pending-games", type=int, default=20_000)
    p.add_argument("--seed", default="0")
    # Worker processes for dealing games, and engine runs at once. The numbers
    # do not depend on it: only the wall time does.
    p.add_argument("--jobs", type=int, default=len(os.sched_getaffinity(0)))
    args = p.parse_args()

    index = checker.Index(args.decks / "index.jsonl")
    checker.check_commanders(args.decks, index)
    _load_decks(args.decks)
    started = time.monotonic()

    # First the plan: every engine run and every deal, in the order the table
    # prints them. Then both kinds run at once, and the table is read off.
    runs: list[tuple[str, str, bool]] = []  # (deck, criteria file, on the draw)
    deals: list[Deal] = []
    for deck in sorted({q.deck for q in checker.QUESTIONS}):
        compared = [q for q in checker.QUESTIONS if q.deck == deck and not q.pending]
        pending = [q for q in checker.QUESTIONS if q.deck == deck and q.pending]
        for draw in (False, True):
            seat = "draw" if draw else "play"
            for criteria in sorted({q.criteria for q in compared + pending}):
                if any(q.criteria == criteria for q in compared) or (args.decks / criteria).exists():
                    runs.append((deck, criteria, draw))
            seed = f"{args.seed}:{deck}:{seat}"
            # One deal per game count: the questions that cap theirs
            # (checker.Question.games) are dealt their own, shorter run.
            for cap in sorted({q.games for q in compared}, key=lambda c: c or 0):
                batch = tuple(q.name for q in compared if q.games == cap)
                games = min(args.games, cap) if cap else args.games
                run_seed = seed if cap is None else f"{seed}:{cap}"
                deals.append(Deal(deck, draw, batch, games, run_seed))
            if pending:
                names = tuple(q.name for q in pending)
                deals.append(Deal(deck, draw, names, args.pending_games, seed + ":pending"))

    # The engine runs are other processes, so threads are enough to wait on them.
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as threads:
        answered = [
            threads.submit(engine_answers, args.gauntlet, args.decks, deck, criteria, draw)
            for deck, criteria, draw in runs
        ]
        dealt = deal_all(deals, args.decks, args.jobs)
        # Keyed by the file as well as the seat: two files may ask questions
        # of the same name, and each question is held to its own file's answer.
        engine: dict[tuple[str, str, bool], dict] = {}
        for (deck, criteria, draw), future in zip(runs, answered):
            engine[(deck, criteria, draw)] = future.result()

    rows, failures = [], 0
    for deal, hits in zip(deals, dealt):
        seat = "draw" if deal.draw else "play"
        for q in _questions(deal.names):
            answers = engine.get((deal.deck, q.criteria, deal.draw), {})
            if not q.pending:
                if q.name not in answers:
                    raise SystemExit(
                        f"compare: {q.criteria} answered no criterion named {q.name!r}"
                    )
                row, failed = judge(q, answers[q.name], hits[q.name], deal.games)
                failures += failed
            elif q.name in answers:
                row, failed = judge(q, answers[q.name], hits[q.name], deal.games)
                row = row[:-1] + (f"{row[-1]}; the engine answers it, drop pending",)
                failures += failed
            else:
                p_check = hits[q.name] / deal.games
                half = Z_999 * math.sqrt(max(p_check * (1 - p_check), 1e-12) / deal.games)
                row = (q.name, None, p_check, half, f"pending engine ({q.pending})")
            rows.append((deal.deck, seat) + row)

    width = max(len(r[2]) for r in rows)
    print(f"{'deck':8} {'seat':4}  {'question':{width}}  {'engine':>9}  {'checker':>9}  {'99.9% ±':>8}  verdict")
    for deck, seat, name, pe, pc, half, verdict in rows:
        engine_cell = f"{pe:9.4%}" if pe is not None else f"{'-':>9}"
        print(f"{deck:8} {seat:4}  {name:{width}}  {engine_cell}  {pc:9.4%}  {half:8.4%}  {verdict}")
    elapsed = time.monotonic() - started
    capped = sorted({q.games for q in checker.QUESTIONS if q.games and not q.pending})
    print(
        f"\n{args.games:,} games per deck and seat"
        + "".join(f" ({min(c, args.games):,} for questions capped at {c:,})" for c in capped)
        + f" ({args.pending_games:,} for pending questions), seed {args.seed!r}, {elapsed:.1f}s"
    )
    if failures:
        print(f"FAIL: {failures} answer(s) outside the 99.9% interval")
        return 1
    print("OK: every answer is inside its 99.9% interval")
    return 0


if __name__ == "__main__":
    sys.exit(main())
