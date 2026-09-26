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
    # The kinds of mana this land can pay a symbol with. Scryfall's `produces`
    # at face value, except where the rules say it makes less - see
    # `_mana_of` - or where it fetches, which `load_library` fills in from the
    # rest of the deck.
    produces: frozenset[str]
    # ASSUMPTION (README "Mana, as a gate", HANDS.md hand 8): a land tagged
    # `conditional-tapland` - shocklands, Mystic Sanctuary, Argoth, Sea Gate -
    # is taken to enter tapped, the pessimistic half of the pilot's choice. Real
    # Magic lets you pay 2 life for a shockland.
    enters_tapped: bool
    # Whether it pays for anything at all. Maze of Ith has no mana ability, so
    # it is a land drop and nothing more (HANDS.md hand 39).
    makes_mana: bool = True
    # How many turns it makes mana for, counting the one it is played on, or
    # None for ever. Urza's Saga: chapter I on the turn it lands, II and III
    # after the next two draw steps, and III sacrifices it (CR 714.4) - three.
    lasts: int | None = None
    # What a fetchland searches for, as (land types, must be basic, enters
    # tapped), or None. Resolved against the deck in `load_library`.
    fetch: tuple[frozenset[str], bool, bool] | None = None

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


BASIC_TYPES = ("Plains", "Island", "Swamp", "Mountain", "Forest")
WUBRG = frozenset("WUBRG")


def _mana_of(oracle: str, listed: frozenset[str]) -> frozenset[str]:
    """The kinds of mana a land makes with no strings attached.

    Scryfall's `produced_mana` lists every colour a card could ever make. A
    colour behind a spending restriction (Castle Doom: "Spend this mana only
    to cast an artifact spell", CR 106.6), an activation condition (Spire of
    Industry: "Activate only if you control an artifact") or an opponent's
    lands (Exotic Orchard: "could produce", CR 106.7) cannot pay for an
    ordinary spell on every turn, so only the abilities without one count.
    """
    lines = [l for l in oracle.split("\n") if "Add" in l]

    def strings_attached(line: str) -> bool:
        return any(
            s in line for s in ("Spend this mana only", "Activate only if", "could produce")
        )

    if not any(strings_attached(l) for l in lines):
        return listed
    made: set[str] = set()
    for line in lines:
        if strings_attached(line):
            continue
        made.update(re.findall(r"\{([WUBRGC])\}", line))
        if "any color" in line:
            made.update(WUBRG)
    return listed & frozenset(made)


def _fetch_of(oracle: str) -> tuple[frozenset[str], bool, bool] | None:
    """What a land that sacrifices itself to search the library finds:
    "{T}, Pay 1 life, Sacrifice this land: Search your library for a Forest or
    Island card, put it onto the battlefield, then shuffle." Only a search the
    land pays for without mana."""
    m = re.search(
        r"^([^:\n]*Sacrifice[^:\n]*): Search your library for ([^.]*?) card, put it onto the "
        r"battlefield( tapped)?",
        oracle,
        re.M,
    )
    if not m or re.sub(r"\{T\}", "", m.group(1)).count("{"):
        return None
    wanted = m.group(2)
    types = frozenset(t for t in BASIC_TYPES if t in wanted)
    return types, "basic" in wanted, bool(m.group(3))


def _make_card(record: dict, categories: tuple[str, ...]) -> Card:
    faces = record.get("faces") or [{"type_line": record["type_line"]}]
    front_is_land = "Land" in faces[0]["type_line"]
    mdfc_land = record.get("layout") == "modal_dfc" and any(
        "Land" in face["type_line"] for face in faces
    )
    tags = set(record.get("tags", []))
    oracle = record.get("oracle", "")
    listed = frozenset(record.get("produces", []))
    playable = front_is_land or mdfc_land
    fetch = _fetch_of(oracle) if playable and not listed else None
    # Urza's Saga: a Saga land lasts as many turns as it has chapters.
    chapters = re.findall(r"^(I|II|III|IV|V|VI)\b", oracle, re.M)
    lasts = (
        ["I", "II", "III", "IV", "V", "VI"].index(chapters[-1]) + 1
        if "Saga" in faces[0]["type_line"] and chapters
        else None
    )
    return Card(
        name=record["name"],
        categories=categories,
        type_line=record["type_line"],
        playable_land=playable,
        produces=_mana_of(oracle, listed),
        enters_tapped=bool(tags & {"tapland", "conditional-tapland"}),
        # A land Scryfall lists as making nothing, and which does not fetch,
        # makes nothing: Maze of Ith.
        makes_mana=bool(listed) or fetch is not None,
        lasts=lasts,
        fetch=fetch,
    )


def _resolve_fetches(library: list[Card]) -> list[Card]:
    """A fetchland as the lands it finds in this deck.

    Cracked the turn it is played, it puts a land onto the battlefield, and
    one that enters untapped pays that same turn: so a fetchland pays any
    colour an untapped land it could find makes. One that says "tapped" - or
    that can find only lands entering tapped - is a tapped land of every
    colour it could find.

    ASSUMPTION (README "Lands that make other than they list"): one such land
    is still in the library to find. Real Magic can run out - two Mistys and
    one Forest pay {G}{G} once - and the documented model does not count that.
    A shockland is one of the lands entering tapped, by the hand-8 assumption.
    """
    resolved = []
    for card in library:
        if card.fetch is None:
            resolved.append(card)
            continue
        types, basic, tapped = card.fetch
        found = [
            c
            for c in library
            if c.playable_land
            and "Land" in c.type_line.split("//")[0]
            and c.makes_mana
            and c.fetch is None
            and (not basic or "Basic" in c.type_line)
            and (not types or any(t in c.type_line.split("//")[0] for t in types))
        ]
        untapped = [c for c in found if not c.enters_tapped]
        if not tapped and untapped:
            found = untapped
        else:
            tapped = True
        resolved.append(
            Card(
                name=card.name,
                categories=card.categories,
                type_line=card.type_line,
                playable_land=True,
                produces=frozenset().union(*(c.produces for c in found)),
                enters_tapped=tapped,
                makes_mana=bool(found),
            )
        )
    return resolved


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
    return _resolve_fetches(library)


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

        * a land that makes mana for only so many turns (Urza's Saga, three)
          must be played late enough to still be there: no earlier than
          `turn - lasts + 1`.

        ASSUMPTION (README): a land is one mana. Izzet Boilerworks and Simic
        Growth Chamber tap for two in real Magic; `can_cast` is "a matching
        over lands" (decks/loam.criteria.toml), one land per symbol.
        A generic symbol is paid by any land that makes mana. A land with no
        mana ability (Maze of Ith) pays nothing. A fetchland pays what the land
        it finds would - see `_resolve_fetches`. Exotic Orchard pays generic:
        ASSUMPTION (README) that an opponent has a land by then.
        Mana rocks and creatures are not sources: "can_cast is a matching over
        LANDS".
        """
        generic, pips = parse_cost(cost)
        need = generic + len(pips)
        if need == 0:
            return True
        lands = [(t, c) for t, c in self.lands_in_hand(turn) if c.makes_mana]
        if len(lands) < need:
            return False
        for chosen in itertools.combinations(lands, need):
            if _schedulable(chosen, turn) and _pays(chosen, pips):
                return True
        return False


def _schedulable(lands: tuple[tuple[int, Card], ...], turn: int) -> bool:
    """Can these lands each take a distinct land drop in [arrival, deadline]?
    Earliest-deadline-first over unit slots is exact for this. A land that
    stops making mana is a later release: the first turn it could go down and
    still pay on `turn`."""
    jobs = sorted(
        (
            arrival if card.lasts is None else max(arrival, turn - card.lasts + 1),
            turn - 1 if card.enters_tapped else turn,
        )
        for arrival, card in lands
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


def _one_mana_by_5(g: Game) -> bool:
    # { turn = 5, can_cast = "{1}" }
    return g.can_cast(5, "{1}")


def _one_green_by_5(g: Game) -> bool:
    # { turn = 5, can_cast = "{1}{G}" }
    return g.can_cast(5, "{1}{G}")


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
        "control: {1}{G} payable by turn 5, no Loam asked",
        _one_green_by_5,
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
        "route 1 control: {1} payable by turn 5, no Lantern asked",
        _one_mana_by_5,
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
