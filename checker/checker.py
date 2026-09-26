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

import functools
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
    # Printed cost, oracle text and colour identity, as the index holds them.
    # Read by the line (`line_path`) and nothing before it.
    mana_cost: str = ""
    oracle: str = ""
    identity: frozenset[str] = frozenset()

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
        mana_cost=record.get("mana_cost") or faces[0].get("mana_cost") or "",
        oracle=oracle,
        identity=frozenset(record.get("ci") or []),
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


def commander_cards(decklist: Path, index: Index) -> tuple[Card, ...]:
    """The commanders as cards, for a line that casts them."""
    return tuple(
        _make_card(index.card(name), ("Commander",)) for name, _ in commanders(decklist, index)
    )


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
    # The command zone: castable from turn 1, never drawn. Only a line reads it.
    commanders: tuple[Card, ...] = ()
    # The whole library the deal came from, so a tutor knows what is left.
    library: list[Card] = field(default_factory=list)
    _line_cache: dict = field(default_factory=dict)

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


@functools.lru_cache(maxsize=None)
def _parse_cost_cached(cost: str) -> tuple[int, tuple[str, ...]]:
    generic, pips = _parse_cost(cost)
    return generic, tuple(pips)


def parse_cost(cost: str) -> tuple[int, list[str]]:
    generic, pips = _parse_cost_cached(cost)
    return generic, list(pips)


def _parse_cost(cost: str) -> tuple[int, list[str]]:
    generic, pips = 0, []
    for sym in re.findall(r"\{([^}]*)\}", cost):
        if sym.isdigit():
            generic += int(sym)
        elif sym in ("W", "U", "B", "R", "G", "C"):
            pips.append(sym)
        else:
            raise ValueError(f"cost symbol {{{sym}}} is not a fixed amount")
    return generic, pips


# --- The line: what a declared [casting] list casts ---------------------------
#
# One model for every question that casts: the commander from the command
# zone, a tutor that fetches to hand, and the rocks and dorks of ADR 0018 and
# HANDS.md hands 26-33 (checker/test_rocks.py holds those hands against it).
# Written from the README, the ADR and the Comprehensive Rules:
#
# * A line is a list of entries, each a tuple of card names, read from the top:
#   the first entry with a card in hand the pool can still pay for is cast,
#   and the line is read again from the top after every cast, so after a cast
#   that grew the pool (ADR 0018) or put a card in hand (HANDS.md hand 36). A
#   card the line does not name is never cast. A tie inside an entry goes to
#   the cheaper cost, then to the order the entry names them. ASSUMPTION: the
#   engine breaks that last tie by decklist order; every entry here names its
#   cards in decklist order.
# * What one turn casts is one bill (README "Mana, as a budget"), and mana does
#   not carry over. The lands are the gate's: whichever lands pay, asked
#   afresh each turn, as `can_cast` asks it (README: "the gate assumes
#   whichever land pays"; nobody plays their lands badly).
# * A non-creature mana source adds mana the turn it is cast, but that mana
#   pays only for spells cast after it that turn and never for itself: its
#   cost is paid while it is still a spell (CR 601.2g-h). So a turn's bill is
#   one matching in which each spell sees only the sources already there when
#   it was cast (HANDS.md hand 27).
# * A creature source is summoning-sick (CR 302.6): it adds from the turn
#   after it was cast.
# * How much a source makes is read from its oracle text, the way ADR 0018's
#   two standard-library entries read it: a single-faced non-land whose text
#   says "{T}: Add" and none of the conditional, delayed or restricted
#   wordings adds 1, and "{T}: Add {C}{C}." adds 2. Its colours are
#   `produces`, with "your commander's color identity" (Arcane Signet) narrowed
#   to the commanders'. So Sol Ring is {C}{C}, Mind Stone {C}, a Talisman one of
#   {C} and its two colours, Birds any colour, Elvish Mystic {G}.
# * Fellwar Stone makes nothing: "a land an opponent controls could produce",
#   and a north star has no opponent (CR 106.7). Lotus Cobra's mana is a
#   landfall trigger rather than a tap, so it is no source. `unmodelled_sources`
#   names both. Improvise and other cost reducers are not modelled: every spell
#   pays its printed cost.
# * A tutor that fetches to hand takes a card the library still holds: one
#   the deck has more copies of than have been seen or fetched. The shuffle
#   after it leaves the rest a uniformly random order of what is left, which is
#   this deal with that copy taken out: later draws move up by one.

_NOT_A_PLAIN_TAP = (
    "enters tapped",
    "doesn't untap",
    "Spend this mana only",
    "can't be spent",
    "Activate only",
    "could produce",
    ", {T}: Add",
    "for each",
    "an amount of",
    "{X}",
)


@dataclass(frozen=True)
class Source:
    amount: int
    palette: frozenset[str]
    sick: bool  # a creature: its mana starts the turn after it is cast


def mana_source(card: Card, identity: frozenset[str] = frozenset()) -> Source | None:
    """What a non-land permanent adds once a line has cast it, or None."""
    text = card.oracle
    if card.playable_land or "//" in card.type_line or "{T}: Add" not in text:
        return None
    if any(wording in text for wording in _NOT_A_PLAIN_TAP):
        return None
    amount = 2 if "{T}: Add {C}{C}." in text else 1
    palette = card.produces
    if "commander's color identity" in text:
        palette = palette & identity
    return Source(amount, palette, "Creature" in card.type_line)


def unmodelled_sources(cards, identity: frozenset[str] = frozenset()) -> list[str]:
    """Permanents whose text adds mana and which are counted as making none:
    Fellwar Stone, Lotus Cobra. A line that casts one gives a lower bound, and
    should name the card that makes it one. (Not `produces`, which
    `_mana_of` has already emptied for the Stone.)"""
    return sorted(
        {
            c.name
            for c in cards
            if not c.playable_land
            and "//" not in c.type_line
            and not any(t in c.type_line for t in ("Instant", "Sorcery"))
            and re.search(r"\badd\b", c.oracle, re.I)
            and mana_source(c, identity) is None
        }
    )


Unit = tuple[int, frozenset[str]]  # (the first bill position it may pay, palette)
Cost = tuple[int, tuple[str, ...]]


def _settles(units: list[Unit], bill: list[Cost]) -> bool:
    """Can every symbol of this turn's bill have its own unit of mana, each
    spell paid only from units that existed before it was cast? A bipartite
    matching: ADR 0018's "matching with nested supply"."""
    demands: list[tuple[int, str | None]] = []
    for pos, (generic, pips) in enumerate(bill):
        demands += [(pos, pip) for pip in pips] + [(pos, None)] * generic
    if len(demands) > len(units):
        return False
    owner = [-1] * len(units)

    def fits(d: int, u: int) -> bool:
        pos, pip = demands[d]
        avail, palette = units[u]
        return avail <= pos and (pip is None or pip in palette)

    def augment(d: int, tried: set[int]) -> bool:
        for u in range(len(units)):
            if u not in tried and fits(d, u):
                tried.add(u)
                if owner[u] < 0 or augment(owner[u], tried):
                    owner[u] = d
                    return True
        return False

    return all(augment(d, set()) for d in range(len(demands)))


def _land_sets(g: Game, turn: int, size: int) -> list[tuple[frozenset[str], ...]]:
    """Every set of `size` lands that could all be untapped on `turn` (the
    gate's schedule), as their palettes, one per distinct palette multiset.
    If no set that large could be, the largest that could. Cached."""
    key = ("lands", turn, size)
    if key not in g._line_cache:
        lands = [(t, c) for t, c in g.lands_in_hand(turn) if c.makes_mana]
        found: dict = {}
        for k in range(min(size, len(lands)), -1, -1):
            for chosen in itertools.combinations(lands, k):
                if _schedulable(chosen, turn):
                    palettes = tuple(sorted((c.produces for _, c in chosen), key=sorted))
                    found.setdefault(palettes, palettes)
            # Schedulable sets are a matroid, so any set that pays extends to
            # one of the largest: stop at the first size that has any.
            if found:
                break
        g._line_cache[key] = list(found)
    return g._line_cache[key]


def _line_pays(g: Game, turn: int, units: list[Unit], bill: list[Cost]) -> bool:
    """Could this turn's lands, whichever pay, and `units` settle `bill`?"""
    need = sum(generic + len(pips) for generic, pips in bill)
    for palettes in _land_sets(g, turn, need):
        if len(palettes) + len(units) < need:
            return False  # every set in the list is the same size
        if _settles([(0, p) for p in palettes] + units, bill):
            return True
    return False


@dataclass
class Turn:
    """One turn of a line: what it cast, and the pool it cast from."""

    number: int
    cast: list[Card]
    units: list[Unit]
    bill: list[Cost]
    game: Game

    def casts(self, name: str) -> bool:
        return any(c.name == name for c in self.cast)

    def left_pays(self, cost: str) -> bool:
        """`can_cast` beside the line: could what the line left, unspent rock
        mana included, still pay `cost` this turn?"""
        return _line_pays(self.game, self.number, self.units, self.bill + [_parse_cost_cached(cost)])


class LinePath(list):
    """The turns of one line, from turn 1."""

    def cast_by(self, name: str, turn: int) -> bool:
        return any(t.casts(name) for t in self[:turn])

    def first_cast(self, name: str) -> int | None:
        return next((t.number for t in self if t.casts(name)), None)


Line = tuple[tuple[str, ...], ...]


def _mana_value(card: Card) -> int:
    generic, pips = _parse_cost_cached(card.mana_cost)
    return generic + len(pips)


def line_path(
    game: Game, line: Line, last_turn: int, fetches: dict[str, tuple[str, ...]] | None = None
) -> LinePath:
    """Play `line` out through `last_turn`. `fetches` maps a card to the cards
    it puts into your hand from the library when cast. Cached on the game."""
    fetches = fetches or {}
    key = ("path", line, last_turn, tuple(sorted(fetches.items())))
    if key in game._line_cache:
        return game._line_cache[key]
    named = {n for entry in line for n in entry}
    identity = frozenset().union(*(c.identity for c in game.commanders))
    order = {n: j for entry in line for j, n in enumerate(entry)}
    g = game
    hand = [c for c in game.commanders if c.name in named]
    seen_so_far = 0
    taken: list[str] = []  # names seen or fetched: not in the library any more
    sources: list[Source] = []
    path = LinePath()
    for t in range(1, last_turn + 1):
        new = g.seen(t)[seen_so_far:]
        seen_so_far = g.seen_count(t)
        taken += [c.name for c in new]
        hand += [c for c in new if c.name in named]
        units: list[Unit] = [(0, s.palette) for s in sources for _ in range(s.amount)]
        bill: list[Cost] = []
        cast: list[Card] = []
        while True:
            chosen = None
            for entry in line:
                options = [c for c in hand if c.name in entry]
                if len(options) > 1:
                    options.sort(key=lambda c: (_mana_value(c), order[c.name]))
                for c in options:
                    cost = _parse_cost_cached(c.mana_cost)
                    if _line_pays(g, t, units, bill + [cost]):
                        chosen = (c, cost)
                        break
                if chosen:
                    break
            if not chosen:
                break
            card, cost = chosen
            bill.append(cost)
            cast.append(card)
            hand.remove(card)
            src = mana_source(card, identity)
            if src is not None:
                if not src.sick:
                    units += [(len(bill), src.palette)] * src.amount
                sources.append(src)
            for wanted in fetches.get(card.name, ()):
                copies = [c for c in g.library if c.name == wanted]
                if len(copies) <= taken.count(wanted):
                    continue  # the library holds none: the search finds nothing
                taken.append(wanted)
                hand.append(copies[0])
                below = next(
                    (i for i in range(seen_so_far, len(g.cards)) if g.cards[i].name == wanted),
                    None,
                )
                if below is not None:
                    cards = g.cards[:below] + g.cards[below + 1 :]
                    g = Game(
                        cards,
                        g.on_the_draw,
                        g.library_size - 1,
                        commanders=g.commanders,
                        library=g.library,
                    )
        path.append(Turn(t, cast, units, bill, g))
    game._line_cache[key] = path
    return path


def line_holds(game: Game, line: Line, last_turn: int, holds: Callable[[LinePath], bool]) -> bool:
    return holds(line_path(game, line, last_turn))


def earliest_cast(game: Game, line: Line, name: str, last_turn: int) -> int | None:
    """The first turn `line` casts `name`, or None if it has not by `last_turn`."""
    return line_path(game, line, last_turn).first_cast(name)


def _gate_turn(g: Game, cost: str, deepest: int) -> int | None:
    """The first turn the lands alone could pay `cost`: the gate."""
    key = ("gate", cost, deepest)
    if key not in g._line_cache:
        g._line_cache[key] = next((t for t in range(1, deepest + 1) if g.can_cast(t, cost)), None)
    return g._line_cache[key]


def _cast_by(line: Line, name: str, turn: int, deepest: int) -> Callable[[Game], bool]:
    """`name` cast by `turn` under `line`, played through `deepest`.

    A shortcut that changes no answer, only the time: when the line names a
    commander first, the gate is where it is cast unless one of the line's
    other cards arrived before then. The commander is in hand from turn 1; on
    the gate's turn the first entry is asked before anything else, out of a
    pool holding at least those lands; and before it, a line with nothing else
    in hand is the gate's line."""
    others = {n for entry in line for n in entry} - {name}

    def ask(g: Game) -> bool:
        commander = next((c for c in g.commanders if c.name == name), None)
        if line[0] == (name,) and commander is not None:
            gate = _gate_turn(g, commander.mana_cost, deepest)
            early = min((gate or deepest + 1) - 1, deepest)
            if early < 1 or not any(c.name in others for c in g.seen(early)):
                return gate is not None and gate <= turn
        return line_path(g, line, deepest).cast_by(name, turn)

    return ask


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
    # The engine ticket this question waits on, or None when the engine
    # answers it today. compare.py reports a pending question's checker number
    # and fails nothing on it; deleting this argument, once the criterion
    # exists in `criteria`, makes it an ordinary comparison.
    pending: str | None = None


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


LOAM, SEEKER = "Life from the Loam", "Spellseeker"
# [[effect]] match = 'name:"Spellseeker"', on = "cast",
#            fetch = ['name:"Life from the Loam"'], to = "hand"
# [casting] prefer = ['name:"Life from the Loam"', 'name:"Spellseeker"']
#
# Spellseeker's enters trigger searches the library for an instant or sorcery
# with mana value 2 or less and puts it into your hand; Loam is a sorcery at
# mana value 2, and the only one the effect names. The line is read again
# after the fetch (HANDS.md hand 36), and a Loam cast resolves into the
# graveyard (CR 608.2n); no other route to the yard is modelled (README
# "Zones"). Everything else is `line_path`.
LOAM_CAST_LINE = ((LOAM,), (SEEKER,))
SEEKER_FETCHES = {SEEKER: (LOAM,)}


def _loam_in_graveyard_by_casting_by_5(g: Game) -> bool:
    # { turn = 5, query = 'name:"Life from the Loam"', zone = "graveyard", min = 1 }
    return line_path(g, LOAM_CAST_LINE, 5, SEEKER_FETCHES).cast_by(LOAM, 5)


def _seeker_and_loam_cast_by_5(g: Game) -> bool:
    # { turn = 5, cast = 'name:"Spellseeker"', min = 1 }
    # { turn = 5, cast = 'name:"Life from the Loam"', min = 1 }
    path = line_path(g, LOAM_CAST_LINE, 5, SEEKER_FETCHES)
    return path.cast_by(LOAM, 5) and path.cast_by(SEEKER, 5)


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


def _commander_cast_by(turn: int, commander: tuple[str, str]) -> Callable[[Game], bool]:
    """The commander, from the command zone, has been cast by `turn`, by a line
    that names only it. The commander is always available and never drawn, so
    this is the gate on its cost - `_cast_by` says why.

    ASSUMPTION (the ticket, #78): casting it once is enough, so commander tax -
    {2} more for each earlier cast from the command zone - never comes up.
    """
    name, _ = commander
    return _cast_by(((name,),), name, turn, turn)


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
        6,  # turn 5, and one card deeper for the Loam a fetch takes out
    ),
    Question(
        "loam",
        "loam-cast.criteria.toml",
        "Spellseeker and Life from the Loam both cast by turn 5",
        _seeker_and_loam_cast_by_5,
        6,
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
        _commander_cast_by(4, LANTERN_COMMANDER),
        4,
    ),
    Question(
        "loam",
        "loam-commander.criteria.toml",
        "commander cast by turn 5",
        # { turn = 5, cast = 'name:"Borborygmos and Fblthp"', min = 1 }
        # with 'name:"Borborygmos and Fblthp"' the only entry in [casting] prefer
        _commander_cast_by(5, LOAM_COMMANDER),
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


# --- Rocks and dorks in the line (ADR 0018), pending the engine (#93) --------
#
# The commander and the rocks or dorks, in one line. The engine cannot answer
# these until its budget reads `adds` (#93), so they are `pending`: compare.py
# reports them and fails nothing. The lands-only pair beside each is the gate,
# which is what the same line reads with no source in it.

RASHMI, BORBORYGMOS = LANTERN_COMMANDER[0], LOAM_COMMANDER[0]
# The commander first, then the rocks: cast Rashmi the moment the pool pays,
# and otherwise grow the pool. Coloured rocks before Mind Stone. Fellwar Stone
# is left out, because casting it costs two mana and makes none.
LANTERN_ROCK_LINE: Line = (
    (RASHMI,),
    ("Sol Ring",),
    ("Arcane Signet",),
    ("Talisman of Creativity",),
    ("Talisman of Curiosity",),
    ("Talisman of Impulse",),
    ("Mind Stone",),
)
# ADR 0018's estimate put the rocks first; with that order a rock can take the
# mana the commander needed, so this line can lose games the gate wins.
LANTERN_ROCKS_FIRST_LINE: Line = LANTERN_ROCK_LINE[1:] + LANTERN_ROCK_LINE[:1]
# Lotus Cobra is left out: it would cost {1}{G} and count as making nothing.
LOAM_DORK_LINE: Line = ((BORBORYGMOS,), ("Birds of Paradise", "Elvish Mystic"))

ROCK_QUESTIONS: list[Question] = (
    [
        Question(
            "lantern",
            "lantern-rocks.criteria.toml",
            f"{RASHMI} castable by turn {t}, lands only",
            _cast_by(((RASHMI,),), RASHMI, t, 5),
            5,
            pending="#93",
        )
        for t in (4, 5)
    ]
    + [
        Question(
            "lantern",
            "lantern-rocks.criteria.toml",
            f"{RASHMI} cast by turn {t}, rocks in the line",
            _cast_by(LANTERN_ROCK_LINE, RASHMI, t, 5),
            5,
            pending="#93",
        )
        for t in (4, 5)
    ]
    + [
        Question(
            "lantern",
            "lantern-rocks.criteria.toml",
            f"{RASHMI} cast by turn 5, rocks first in the line",
            _cast_by(LANTERN_ROCKS_FIRST_LINE, RASHMI, 5, 5),
            5,
            pending="#93",
        ),
    ]
    + [
        Question(
            "loam",
            "loam-rocks.criteria.toml",
            f"{BORBORYGMOS} castable by turn {t}, lands only",
            _cast_by(((BORBORYGMOS,),), BORBORYGMOS, t, 5),
            5,
            pending="#93",
        )
        for t in (4, 5)
    ]
    + [
        Question(
            "loam",
            "loam-rocks.criteria.toml",
            f"{BORBORYGMOS} cast by turn {t}, dorks in the line",
            _cast_by(LOAM_DORK_LINE, BORBORYGMOS, t, 5),
            5,
            pending="#93",
        )
        for t in (4, 5)
    ]
)
QUESTIONS += ROCK_QUESTIONS


# --- Running ------------------------------------------------------------------


def play(
    library: list[Card],
    questions: list[Question],
    on_the_draw: bool,
    games: int,
    seed: str,
    commanders: tuple[Card, ...] = (),
) -> dict[str, int]:
    """Deal `games` games from one seeded shuffle stream and count, per
    question, the games where it held. One deal answers every question, so
    the questions are correlated with each other but each is a fair estimate."""
    rng = random.Random(seed)  # str seeds are hashed deterministically
    deepest = max(q.deepest_turn for q in questions)
    depth = 7 + deepest  # enough for either seat
    hits = {q.name: 0 for q in questions}
    for _ in range(games):
        game = Game(
            rng.sample(library, depth),
            on_the_draw,
            len(library),
            commanders=commanders,
            library=library,
        )
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
    # The pending questions are only reported, so they deal fewer games.
    p.add_argument("--pending-games", type=int, default=100_000)
    args = p.parse_args()
    index = Index(args.decks / "index.jsonl")
    check_commanders(args.decks, index)
    for deck in sorted({q.deck for q in QUESTIONS}):
        library = load_library(args.decks / f"{deck}.txt", index)
        cmdrs = commander_cards(args.decks / f"{deck}.txt", index)
        for pending, games in ((False, args.games), (True, args.pending_games)):
            qs = [q for q in QUESTIONS if q.deck == deck and bool(q.pending) == pending]
            if not qs:
                continue
            seed = f"{args.seed}:{deck}:{args.draw}" + (":pending" if pending else "")
            hits = play(library, qs, args.draw, games, seed, cmdrs)
            for q in qs:
                tag = f"  (pending engine {q.pending}, {games:,} games)" if q.pending else ""
                print(f"{deck:8} {hits[q.name] / games:9.4%}  {q.name}{tag}")


if __name__ == "__main__":
    main()
