"""An independent Monte Carlo checker for the gauntlet's answers.

This file shuffles a real library, deals real games and asks each question of
them in plain Python. It was written from the questions' plain-English meaning,
the Magic rules, and the engine's *documented* assumptions (README.md, HANDS.md)
- never from the engine's source. Where the README states an assumption that
differs from real Magic, this file implements the README and says so in a
comment marked ASSUMPTION.

Standard library only. `compare.py` beside it runs the engine and holds the two
answers against each other; this file knows nothing about the engine.
"""

from __future__ import annotations

import itertools
import json
import random
import re
from dataclasses import dataclass, field
from pathlib import Path
from typing import Callable

# --- Cards ------------------------------------------------------------------


@dataclass(frozen=True)
class Card:
    name: str
    categories: tuple[str, ...]
    type_line: str
    # A card you can put onto the battlefield on a land drop: its front face
    # is a land (this includes Argoth, a meld card), or it is a modal
    # double-faced card with a land face. A *transforming* card whose back face
    # is a land (Search for Azcanta) is not: you cast its front and it becomes a
    # land later, which is the rules and also what VISION.md records the engine
    # fixing in #61.
    playable_land: bool
    # Colours this land can tap for, as Scryfall's `produces` lists them.
    produces: frozenset[str]
    # ASSUMPTION (README "Mana, as a gate", HANDS.md hand 8): a land tagged
    # `conditional-tapland` - shocklands, Mystic Sanctuary, Argoth, Sea Gate -
    # is taken to enter tapped, the pessimistic half of the pilot's choice. Real
    # Magic lets you pay 2 life for a shockland.
    enters_tapped: bool

    def is_named(self, *names: str) -> bool:
        return self.name in names

    def in_category(self, category: str) -> bool:
        return category in self.categories


class Index:
    """decks/index.jsonl: a header line, then `lowercased name<TAB>card json`."""

    def __init__(self, path: Path):
        self._raw: dict[str, str] = {}
        with open(path, encoding="utf-8") as f:
            next(f)  # the header describes the file, not a card
            for line in f:
                key, _, record = line.rstrip("\n").partition("\t")
                self._raw[key] = record

    def card(self, name: str) -> dict:
        try:
            return json.loads(self._raw[name.lower()])
        except KeyError:
            raise SystemExit(f"checker: {name!r} is not in the index") from None


def _make_card(record: dict, categories: tuple[str, ...]) -> Card:
    faces = record.get("faces") or [{"type_line": record["type_line"]}]
    front_is_land = "Land" in faces[0]["type_line"]
    mdfc_land = record.get("layout") == "modal_dfc" and any(
        "Land" in face["type_line"] for face in faces
    )
    tags = set(record.get("tags", []))
    return Card(
        name=record["name"],
        categories=categories,
        type_line=record["type_line"],
        playable_land=front_is_land or mdfc_land,
        produces=frozenset(record.get("produces", [])),
        enters_tapped=bool(tags & {"tapland", "conditional-tapland"}),
    )


_LINE = re.compile(r"^(\d+)x?\s+(.+?)(?:\s+\([^)]*\)\s*\S*)?(?:\s+\*[^*]*\*)?(?:\s+\[(.*)\])?\s*$")


def commanders(decklist: Path, index: Index) -> list[tuple[str, str]]:
    """(name, printed mana cost) of every card the list files under Commander.

    A commander starts the game in the command zone, not the library: it is
    never drawn, and it can be cast from there on any turn its cost is paid.
    """
    found: list[tuple[str, str]] = []
    for raw in decklist.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        m = _LINE.match(line)
        if not m:
            raise SystemExit(f"checker: cannot read decklist line {raw!r}")
        bare = [re.sub(r"\{.*\}$", "", c.strip()) for c in (m.group(3) or "").split(",")]
        if "Commander" in bare:
            record = index.card(m.group(2))
            found.append((record["name"], record["mana_cost"]))
    return found


def load_library(decklist: Path, index: Index) -> list[Card]:
    """The library: every card in the list except commanders and anything the
    list says is outside the deck. One Card object per copy."""
    library: list[Card] = []
    for raw in decklist.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("//"):
            continue
        m = _LINE.match(line)
        if not m:
            raise SystemExit(f"checker: cannot read decklist line {raw!r}")
        qty, name, cats = int(m.group(1)), m.group(2), m.group(3) or ""
        categories = tuple(c.strip() for c in cats.split(",") if c.strip())
        bare = [re.sub(r"\{.*\}$", "", c) for c in categories]
        outside = any(
            c == "Commander" or c in ("Companion", "Sideboard", "Maybeboard")
            for c in bare
        ) or any("{noDeck}" in c for c in categories)
        if outside:
            continue
        card = _make_card(index.card(name), tuple(bare))
        library.extend([card] * qty)
    return library


# --- A dealt game -------------------------------------------------------------


@dataclass
class Game:
    """The top of one shuffled library, and the turn each card arrives on.

    Turn 0 is the opening seven. On the play turn 1 draws nothing, so turn t
    has seen 7 + (t - 1) cards; on the draw it has seen 7 + t. A card in the
    opener arrives on turn 0.
    """

    cards: list[Card]
    on_the_draw: bool
    library_size: int
    _lands_cache: dict = field(default_factory=dict)

    def seen_count(self, turn: int) -> int:
        draws = turn if self.on_the_draw else max(0, turn - 1)
        return 7 + draws

    def seen(self, turn: int) -> list[Card]:
        return self.cards[: self.seen_count(turn)]

    def arrival_turn(self, position: int) -> int:
        if position < 7:
            return 0
        drawn = position - 7 + 1  # which draw brought it, counting from 1
        return drawn if self.on_the_draw else drawn + 1

    def count(self, turn: int, predicate: Callable[[Card], bool]) -> int:
        """How many cards matching `predicate` you have seen by `turn`.
        Nothing is cast in these questions, so seen is held."""
        return sum(1 for c in self.seen(turn) if predicate(c))

    def lands_in_hand(self, turn: int) -> list[tuple[int, Card]]:
        """(first turn it could be played, card) for every playable land seen by
        `turn`. The earliest land drop is turn 1."""
        key = turn
        if key not in self._lands_cache:
            self._lands_cache[key] = [
                (max(1, self.arrival_turn(i)), c)
                for i, c in enumerate(self.seen(turn))
                if c.playable_land
            ]
        return self._lands_cache[key]

    def lands_played(self, turn: int) -> int:
        """Lands on the battlefield by `turn`: one land drop a turn, use it or
        lose it (HANDS.md hand 4). With no declared priority any land will do,
        so play one whenever one is in hand."""
        available = sorted(t for t, _ in self.lands_in_hand(turn))
        played = 0
        for t in range(1, turn + 1):
            if sum(1 for a in available if a <= t) > played:
                played += 1
        return played

    def can_cast(self, turn: int, cost: str) -> bool:
        """Could the lands you had played have paid `cost` on `turn`?

        The gate asks whether *some* sequence of land drops pays - the README's
        "the gate assumes whichever land pays" - so this searches every set of
        lands the cost could use, and asks whether that set could have been
        on the battlefield and untapped on `turn`:

        * each land is played on a turn no earlier than it arrived and no later
          than `turn`, one land per turn;
        * a land that enters tapped makes no mana the turn it is played, so it
          must be played by `turn - 1`.

        ASSUMPTION (README): a land is one mana. Izzet Boilerworks and Simic
        Growth Chamber tap for two in real Magic; `can_cast` is "a matching
        over lands" (decks/loam.criteria.toml), one land per symbol.
        ASSUMPTION (README "Generic takes any land"): a generic symbol is paid
        by any land, including one that `produces` nothing - a fetchland, Maze
        of Ith. A coloured symbol needs a land that produces that colour, so a
        fetchland never pays one. In real Magic a fetchland cracks for a land
        that does; this is the README's reading, not the rules'.
        Mana rocks and creatures are not sources: "can_cast is a matching over
        LANDS".
        """
        generic, pips = parse_cost(cost)
        need = generic + len(pips)
        if need == 0:
            return True
        lands = self.lands_in_hand(turn)
        if len(lands) < need:
            return False
        for chosen in itertools.combinations(lands, need):
            if _schedulable(chosen, turn) and _pays(chosen, pips):
                return True
        return False


def _schedulable(lands: tuple[tuple[int, Card], ...], turn: int) -> bool:
    """Can these lands each take a distinct land drop in [arrival, deadline]?
    Earliest-deadline-first over unit slots is exact for this."""
    jobs = sorted(
        (arrival, turn - 1 if card.enters_tapped else turn) for arrival, card in lands
    )
    pending: list[int] = []
    i = 0
    for slot in range(1, turn + 1):
        while i < len(jobs) and jobs[i][0] <= slot:
            pending.append(jobs[i][1])
            i += 1
        if pending:
            pending.sort()
            if pending.pop(0) < slot:
                return False
    return i == len(jobs) and not pending


def _pays(lands: tuple[tuple[int, Card], ...], pips: list[str]) -> bool:
    """Exactly len(lands) symbols to pay, one land each: give every coloured
    pip a distinct land that makes it; the rest pay generic."""
    if not pips:
        return True
    cards = [c for _, c in lands]
    for assignment in itertools.permutations(range(len(cards)), len(pips)):
        if all(pip in cards[j].produces for pip, j in zip(pips, assignment)):
            return True
    return False


def parse_cost(cost: str) -> tuple[int, list[str]]:
    generic, pips = 0, []
    for sym in re.findall(r"\{([^}]*)\}", cost):
        if sym.isdigit():
            generic += int(sym)
        elif sym in ("W", "U", "B", "R", "G", "C"):
            pips.append(sym)
        else:
            raise ValueError(f"cost symbol {{{sym}}} is not a fixed amount")
    return generic, pips


# --- Questions ----------------------------------------------------------------
#
# Each question is the engine's criterion name (so compare.py can find the
# engine's answer), the criteria file it lives in, and a function of one dealt
# game. They are written from what the question *means*; the TOML clauses are
# quoted beside each so a reader can check the translation.


@dataclass(frozen=True)
class Question:
    deck: str  # decklist stem under decks/
    criteria: str  # criteria file under decks/
    name: str  # the criterion's name, exactly as the file spells it
    ask: Callable[[Game], bool]
    deepest_turn: int


LOAM_TWO_DROPS = (
    "Aftermath Analyst",
    "Life from the Loam",
    "Malevolent Rumble",
    "Midnight Tilling",
)
LANTERN_BATTLEFIELD_TUTORS = ("Tezzeret the Seeker", "Whir of Invention")


def _loam_castable_by_5(g: Game) -> bool:
    # { turn = 5, query = 'name:"Life from the Loam"', min = 1 }
    # { turn = 5, can_cast = "{1}{G}" }
    return g.count(5, lambda c: c.is_named("Life from the Loam")) >= 1 and g.can_cast(
        5, "{1}{G}"
    )


def _loam_in_graveyard_by_casting_by_5(g: Game) -> bool:
    # [casting] prefer = ['name:"Life from the Loam"']
    # { turn = 5, query = 'name:"Life from the Loam"', zone = "graveyard", min = 1 }
    #
    # Played out a turn at a time rather than asked as a joint. Life from the
    # Loam is a sorcery, and a sorcery that resolves is put into its owner's
    # graveyard (CR 608.2n), so it is in the yard by turn 5 exactly when the
    # line cast it on some turn up to 5. The line casts it on the first turn
    # it is in hand and the lands in play could pay {1}{G}; nothing else in the
    # line competes for the mana, and a card you have cast is not in hand to
    # be cast again. No other route to the yard is modelled (README: "Zones").
    for turn in range(1, 6):
        held = g.count(turn, lambda c: c.is_named("Life from the Loam")) >= 1
        if held and g.can_cast(turn, "{1}{G}"):
            return True  # cast, resolved, in the graveyard from here on
    return False


def _loam_two_drop_and_mana_t3(g: Game) -> bool:
    # { turn = 3, query = <the four {1}{G} Loam Access cards>, min = 1 }
    # { turn = 3, can_cast = "{1}{G}" }
    return g.count(3, lambda c: c.is_named(*LOAM_TWO_DROPS)) >= 1 and g.can_cast(
        3, "{1}{G}"
    )


def _loam_access_and_three_lands_t3(g: Game) -> bool:
    # { turn = 3, query = 'cat:"Loam Access"', min = 1 }
    # { turn = 3, query = "t:land", zone = "battlefield", min = 3 }
    return g.count(3, lambda c: c.in_category("Loam Access")) >= 1 and g.lands_played(3) >= 3


def _lantern_and_a_mana_t5(g: Game) -> bool:
    # { turn = 5, query = 'name:"Lantern of Insight"', min = 1 }
    # { turn = 5, can_cast = "{1}" }
    return g.count(5, lambda c: c.is_named("Lantern of Insight")) >= 1 and g.can_cast(5, "{1}")


def _battlefield_tutor_drawn_t5(g: Game) -> bool:
    # { turn = 5, query = 'name:"Tezzeret the Seeker" or name:"Whir of Invention"', min = 1 }
    return g.count(5, lambda c: c.is_named(*LANTERN_BATTLEFIELD_TUTORS)) >= 1


def _commander_cast_by(turn: int, cost: str) -> Callable[[Game], bool]:
    """The commander, from the command zone, has been cast by `turn`.

    The commander is always available and never drawn, so the only thing
    between it and the battlefield is mana: it has been cast by `turn` exactly
    when some turn t <= `turn` had untapped lands that pay its cost. Lands in
    play only accumulate and every land in play on t is untapped on t + 1, so
    "payable on some t <= turn" is "payable on `turn`" - which is the gate, with
    the pilot playing whichever lands pay. Nothing else is cast in the line
    this asks about, so nothing else competes for the pool.

    ASSUMPTION (the ticket, #78): casting it once is enough, so commander tax -
    {2} more for each earlier cast from the command zone - never comes up.
    Mana rocks, Rashmi's Treasure and creatures are not sources, as everywhere
    else here.
    """

    def ask(g: Game) -> bool:
        return g.can_cast(turn, cost)

    return ask


# The commanders' costs are written out rather than read from the index so that
# a reader can check them against the card; `main` and compare.py hold them to
# the index's printed cost so they cannot drift.
LANTERN_COMMANDER = ("Rashmi and Ragavan", "{1}{G}{U}{R}")
LOAM_COMMANDER = ("Borborygmos and Fblthp", "{2}{G}{U}{R}")


QUESTIONS: list[Question] = [
    Question(
        "loam",
        "loam.criteria.toml",
        "Loam castable by turn 5, so Loam in the graveyard by turn 5",
        _loam_castable_by_5,
        5,
    ),
    Question(
        "loam",
        "loam-cast.criteria.toml",
        "Life from the Loam in the graveyard by turn 5 (by casting it)",
        _loam_in_graveyard_by_casting_by_5,
        5,
    ),
    Question(
        "loam",
        "loam.criteria.toml",
        "a two-mana Loam Access card and {1}{G} for it, turn 3",
        _loam_two_drop_and_mana_t3,
        3,
    ),
    Question(
        "loam",
        "loam.criteria.toml",
        "Loam Access drawn and three lands in play, turn 3",
        _loam_access_and_three_lands_t3,
        3,
    ),
    Question(
        "lantern",
        "lantern.criteria.toml",
        "route 1: Lantern in hand and a mana for it, turn 5",
        _lantern_and_a_mana_t5,
        5,
    ),
    Question(
        "lantern",
        "lantern.criteria.toml",
        "route 3 naive: either of those two drawn by turn 5",
        _battlefield_tutor_drawn_t5,
        5,
    ),
    Question(
        "lantern",
        "lantern-commander.criteria.toml",
        "commander cast by turn 4",
        # { turn = 4, cast = 'name:"Rashmi and Ragavan"', min = 1 }
        # with 'name:"Rashmi and Ragavan"' the only entry in [casting] prefer
        _commander_cast_by(4, LANTERN_COMMANDER[1]),
        4,
    ),
    Question(
        "loam",
        "loam-commander.criteria.toml",
        "commander cast by turn 5",
        # { turn = 5, cast = 'name:"Borborygmos and Fblthp"', min = 1 }
        # with 'name:"Borborygmos and Fblthp"' the only entry in [casting] prefer
        _commander_cast_by(5, LOAM_COMMANDER[1]),
        5,
    ),
]


def check_commanders(decks: Path, index: Index) -> None:
    """The commanders written above are the ones the decklists name, at the
    costs the index prints."""
    for deck, expected in (("lantern", LANTERN_COMMANDER), ("loam", LOAM_COMMANDER)):
        found = commanders(decks / f"{deck}.txt", index)
        if found != [expected]:
            raise SystemExit(f"checker: {deck}.txt names commanders {found}, expected {[expected]}")


# --- Running ------------------------------------------------------------------


def play(
    library: list[Card],
    questions: list[Question],
    on_the_draw: bool,
    games: int,
    seed: str,
) -> dict[str, int]:
    """Deal `games` games from one seeded shuffle stream and count, per
    question, the games where it held. One deal answers every question, so
    the questions are correlated with each other but each is a fair estimate."""
    rng = random.Random(seed)  # str seeds are hashed deterministically
    deepest = max(q.deepest_turn for q in questions)
    depth = 7 + deepest  # enough for either seat
    hits = {q.name: 0 for q in questions}
    for _ in range(games):
        game = Game(rng.sample(library, depth), on_the_draw, len(library))
        for q in questions:
            if q.ask(game):
                hits[q.name] += 1
    return hits


def main() -> None:
    import argparse

    here = Path(__file__).resolve().parent
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("--decks", type=Path, default=here.parent / "decks")
    p.add_argument("--games", type=int, default=100_000)
    p.add_argument("--seed", default="0")
    p.add_argument("--draw", action="store_true")
    args = p.parse_args()
    index = Index(args.decks / "index.jsonl")
    check_commanders(args.decks, index)
    for deck in sorted({q.deck for q in QUESTIONS}):
        library = load_library(args.decks / f"{deck}.txt", index)
        qs = [q for q in QUESTIONS if q.deck == deck]
        hits = play(library, qs, args.draw, args.games, f"{args.seed}:{deck}:{args.draw}")
        for q in qs:
            print(f"{deck:8} {hits[q.name] / args.games:9.4%}  {q.name}")


if __name__ == "__main__":
    main()
