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
from typing import Callable, Iterator

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
    # A planeswalker's printed loyalty, the counters it enters with (CR
    # 306.5b), or None for anything else.
    loyalty: int | None = None
    # Scryfall's oracle tags, as the index holds them: `otag:` in a file. A
    # declared land drop reads `tapland`, `conditional-tapland` and `surveil`
    # (see `LandDrop`).
    tags: frozenset[str] = frozenset()
    # Scryfall's `produced_mana` at face value, which is what `produces:` in a
    # file reads: a fetchland lists none.
    listed: frozenset[str] = frozenset()

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
        loyalty=int(faces[0]["loyalty"]) if str(faces[0].get("loyalty", "")).isdigit() else None,
        tags=frozenset(tags),
        listed=listed,
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
                tags=card.tags,
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
    # Lands a spell put into your hand from the library rather than a draw
    # step, as (the first turn it could be played, card). Only a line's mill
    # makes these: see `line_path`.
    kept_lands: tuple[tuple[int, Card], ...] = ()

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
            ] + [(first, c) for first, c in self.kept_lands if first <= turn]
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
#   pays its printed cost, or what the pilot plays it for (below), in full.
# * A planeswalker's loyalty ability can put a card onto the battlefield the
#   turn the line casts it: loyalty abilities are activated any time its
#   controller could cast a sorcery (CR 606.3), which a main phase just after a
#   cast is, once per turn (CR 606.3), and a planeswalker is not a creature, so
#   summoning sickness (CR 302.6) does not stop it. A −X ability costs X
#   loyalty (CR 606.4) and cannot be paid with fewer counters than that (CR
#   606.6), and it enters with its printed loyalty (CR 306.5b). So Tezzeret the
#   Seeker, four loyalty, "−X: Search your library for an artifact card with
#   mana value X or less, put it onto the battlefield", finds the Lantern
#   (mana value 1) with −1 on the turn he resolves (HANDS.md hand 40). What he
#   puts there was never cast (README: `cast` counts castings), and is out of
#   the library the same way a fetch to hand takes it out.
# * A card may be played for what the pilot pays rather than for its printed
#   cost, and then that is what the turn's bill holds (README "A cost the line
#   pays that is not printed"; ADR 0019). Two ways, each read from the card:
#   - Transmute (CR 702.53a): "Transmute [cost]" is "[cost], Discard this
#     card: Search your library for a card with the same mana value as the
#     discarded card, reveal that card, and put it into your hand. Then
#     shuffle. Activate only as a sorcery." It is activated from the hand, so
#     the card is never cast and goes to the graveyard; a main phase with
#     nothing on the stack is sorcery timing, which is where the line plays
#     everything. The README counts it as a casting of the card in a `cast`
#     question, so it is in the turn's `cast` list. Dizzy Spell is printed {U},
#     mana value 1, and its transmute is {1}{U}{U}.
#   - X chosen by the pilot (CR 107.3a, 601.2b): X is announced as the spell
#     is cast and paid as that number. Whir of Invention at X = 1 costs
#     {1}{U}{U}{U}, and "an artifact card with mana value X or less" is then
#     mana value 1 or less, onto the battlefield. Improvise (CR 702.126) is not
#     modelled, as no cost reducer is: the four are paid in full.
#   A tie inside an entry goes to the cheaper of what the line pays.
# * A tutor that fetches to hand takes a card the library still holds: one
#   the deck has more copies of than have been seen or fetched. The shuffle
#   after it leaves the rest a uniformly random order of what is left, which is
#   this deal with that copy taken out: later draws move up by one.
# * A spell that mills (ADR 0017 §2, HANDS.md hands 19 and 20) takes the next
#   cards off the top of the library, the ones the next draws would have
#   found, so later draws move up by that many. Each goes to the graveyard
#   unless the card puts it in your hand: every card of a kind (Wrenn and
#   Seven's lands), or up to so many of the kind it allows, chosen by the
#   pilot's list, first entry first (Rumble's permanent). ASSUMPTION (README,
#   the tutor's tie rule): a tie inside one entry goes to the card the
#   decklist names first. A card kept this way is in hand from then on, so
#   the line may cast it this turn; a land kept this way is played no earlier
#   than the next turn, because this turn's land drop came before the line
#   (ADR 0017: "a land drawn mid-line waits for the next turn's drop").


@dataclass(frozen=True)
class Mill:
    """What casting one card does to the top of the library."""

    cards: int
    # Every card this matches goes to your hand whatever the pilot wants.
    keep_every: Callable[[Card], bool] | None = None
    # Up to this many of the cards `keep_only` allows go to your hand, the
    # first entry of `prefer` that holds one first.
    keep_up_to: int = 0
    keep_only: Callable[[Card], bool] | None = None
    prefer: tuple[Callable[[Card], bool], ...] = ()

    def kept(self, top: list[Card], decklist: list[Card]) -> list[int]:
        """Which of `top` (by position) go to your hand."""
        if self.keep_every is not None:
            return [i for i, c in enumerate(top) if self.keep_every(c)]
        kept: list[int] = []
        allowed = self.keep_only or (lambda c: True)
        for wants in self.prefer:
            if len(kept) >= self.keep_up_to:
                break
            options = [
                i for i, c in enumerate(top) if i not in kept and allowed(c) and wants(c)
            ]
            options.sort(key=lambda i: decklist.index(top[i]))
            kept += options[: self.keep_up_to - len(kept)]
        return kept

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
    # What a cast put onto the battlefield from the library this turn, and the
    # names seen or taken out of the library by its end.
    put: list[Card] = field(default_factory=list)
    taken: list[str] = field(default_factory=list)
    # What this turn's spells milled into the graveyard.
    milled: list[Card] = field(default_factory=list)

    def casts(self, name: str) -> bool:
        return any(c.name == name for c in self.cast)

    def puts_in_graveyard(self, name: str) -> bool:
        """An instant or sorcery cast this turn resolves into the graveyard
        (CR 608.2n); a milled card is put there."""
        return any(
            c.name == name and any(t in c.type_line for t in ("Instant", "Sorcery"))
            for c in self.cast
        ) or any(c.name == name for c in self.milled)

    def left_pays(self, cost: str) -> bool:
        """`can_cast` beside the line: could what the line left, unspent rock
        mana included, still pay `cost` this turn?"""
        return _line_pays(self.game, self.number, self.units, self.bill + [_parse_cost_cached(cost)])


class LinePath(list):
    """The turns of one line, from turn 1."""

    def cast_by(self, name: str, turn: int) -> bool:
        return any(t.casts(name) for t in self[:turn])

    def on_battlefield_by(self, name: str, turn: int) -> bool:
        """A permanent the line cast, or one a cast put there, by `turn`.
        Nothing in these lines takes a permanent off the battlefield again."""
        return any(
            any(c.name == name and _is_permanent(c) for c in t.cast + t.put) for t in self[:turn]
        )

    def in_library(self, name: str, turn: int) -> bool:
        """A copy of `name` still in the library at the end of `turn`: neither
        seen nor taken out by a fetch."""
        t = self[turn - 1]
        return sum(1 for c in t.game.library if c.name == name) > t.taken.count(name)

    def first_cast(self, name: str) -> int | None:
        return next((t.number for t in self if t.casts(name)), None)

    def in_graveyard_by(self, name: str, turn: int) -> bool:
        """Put into the graveyard by `turn`. Nothing in a line takes a card
        back out, so this is also "in the graveyard on `turn`"."""
        return any(t.puts_in_graveyard(name) for t in self[:turn])


Line = tuple[tuple[str, ...], ...]


def _mana_value(card: Card) -> int:
    generic, pips = _parse_cost_cached(card.mana_cost)
    return generic + len(pips)


@dataclass(frozen=True)
class Mode:
    """How the pilot plays a card: its transmute, or its spell with X chosen."""

    transmute: bool = False
    x: int | None = None


TRANSMUTE = Mode(transmute=True)


def x_is(x: int) -> Mode:
    return Mode(x=x)


_TRANSMUTE = re.compile(r"^Transmute ((?:\{[^}]*\})+)", re.M)


def play_cost(card: Card, mode: Mode | None) -> str:
    """What the line pays to play `card` in `mode`: the printed cost, the
    transmute cost its text names, or the printed cost with X as chosen."""
    if mode is None:
        return card.mana_cost
    if mode.transmute:
        m = _TRANSMUTE.search(card.oracle)
        if not m:
            raise ValueError(f"{card.name} has no transmute")
        return m.group(1)
    if "{X}" not in card.mana_cost:
        raise ValueError(f"{card.name} has no X to choose")
    return card.mana_cost.replace("{X}", "{%d}" % mode.x)


def _paid(card: Card, modes: dict[str, Mode]) -> int:
    """How much mana the line pays to play `card`."""
    generic, pips = _parse_cost_cached(play_cost(card, modes.get(card.name)))
    return generic + len(pips)


def transmute_finds(source: Card, target: Card) -> bool:
    """Could `source`'s transmute find `target`? The same mana value as the
    discarded card (CR 702.53a), whatever its type."""
    return _TRANSMUTE.search(source.oracle) is not None and _mana_value(target) == _mana_value(source)


_SPELL_X_ONTO_BATTLEFIELD = re.compile(
    r"^Search your library for an? (\w+) card with mana value X or less, put it onto the "
    r"battlefield",
    re.M,
)


_MINUS_X_ONTO_BATTLEFIELD = re.compile(
    r"^[−-]X: Search your library for an? (\w+) card with mana value X or less, put it onto "
    r"the battlefield",
    re.M,
)


def puts_onto_battlefield(source: Card, target: Card, x: int | None = None) -> bool:
    """Could `source`, just cast, put `target` onto the battlefield from the
    library that turn: with a −X loyalty ability, or as a spell whose X the
    pilot chose as `x`? See the line's notes above."""
    if source.loyalty is None:
        if x is None:
            return False
        m = _SPELL_X_ONTO_BATTLEFIELD.search(source.oracle)
        return bool(m) and m.group(1).capitalize() in target.type_line and _mana_value(target) <= x
    m = _MINUS_X_ONTO_BATTLEFIELD.search(source.oracle)
    if not m:
        return False
    return m.group(1).capitalize() in target.type_line and _mana_value(target) <= source.loyalty


def _is_permanent(card: Card) -> bool:
    front = card.type_line.split("//")[0]
    return not any(t in front for t in ("Instant", "Sorcery"))


def line_path(
    game: Game,
    line: Line,
    last_turn: int,
    fetches: dict[str, tuple[str, ...]] | None = None,
    puts: dict[str, tuple[str, ...]] | None = None,
    mills: dict[str, Mill] | None = None,
    modes: dict[str, Mode] | None = None,
    attacks: dict[str, Mill] | None = None,
    landfalls: dict[str, Mill] | None = None,
) -> LinePath:
    """Play `line` out through `last_turn`. `fetches` maps a card to the cards
    it puts into your hand from the library when cast, `puts` to the cards
    its loyalty ability puts onto the battlefield from the library that turn,
    the first of them it can find (`puts_onto_battlefield`), and `mills` to
    what it does to the top of the library. `modes` maps a card to how the
    pilot plays it, where that is not casting it for its printed cost: its
    transmute, whose search is then the one `fetches` names, or its spell with
    X chosen. `attacks` maps a creature to what it mills each time it attacks,
    and `landfalls` a permanent to what it mills each time a land enters
    while it is on the battlefield, each from the turn after the line cast
    it (see "Attack and landfall" among the questions). Cached on the game."""
    fetches = fetches or {}
    puts = puts or {}
    mills = mills or {}
    modes = modes or {}
    attacks = attacks or {}
    landfalls = landfalls or {}
    key = (
        "path",
        line,
        last_turn,
        tuple(sorted(fetches.items())),
        tuple(sorted(puts.items())),
        tuple(sorted(mills.items())),
        tuple(sorted(modes.items(), key=lambda kv: kv[0])),
        tuple(sorted(attacks.items())),
        tuple(sorted(landfalls.items())),
    )
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
    # Permanents the line cast that trigger, and the turn each was cast on.
    in_play: list[tuple[int, Card]] = []
    path = LinePath()

    def mill_top(t: int, mill: Mill, milled: list[Card]) -> None:
        """The next `mill.cards` off the top: the kept ones to hand (a land
        waits for the next turn's drop), the rest into `milled`."""
        nonlocal g, taken
        top = g.cards[seen_so_far : seen_so_far + mill.cards]
        kept = mill.kept(top, g.library)
        taken += [c.name for c in top]
        lands: list[tuple[int, Card]] = []
        for i, c in enumerate(top):
            if i not in kept:
                milled.append(c)
            elif c.playable_land:
                lands.append((t + 1, c))
            elif c.name in named:
                hand.append(c)
        g = Game(
            g.cards[:seen_so_far] + g.cards[seen_so_far + len(top) :],
            g.on_the_draw,
            g.library_size - len(top),
            commanders=g.commanders,
            library=g.library,
            kept_lands=g.kept_lands + tuple(lands),
        )

    for t in range(1, last_turn + 1):
        new = g.seen(t)[seen_so_far:]
        seen_so_far = g.seen_count(t)
        taken += [c.name for c in new]
        hand += [c for c in new if c.name in named]
        units: list[Unit] = [(0, s.palette) for s in sources for _ in range(s.amount)]
        bill: list[Cost] = []
        cast: list[Card] = []
        put: list[Card] = []
        milled: list[Card] = []
        # This turn's land drop, before the line: one land entered if the
        # gate's schedule played one. Each fires every landfall permanent the
        # line cast on an earlier turn.
        entered = g.lands_played(t) - g.lands_played(t - 1)
        for when, permanent in in_play:
            if when < t and permanent.name in landfalls:
                for _ in range(entered):
                    mill_top(t, landfalls[permanent.name], milled)
        while True:
            chosen = None
            for entry in line:
                options = [c for c in hand if c.name in entry]
                if len(options) > 1:
                    options.sort(key=lambda c: (_paid(c, modes), order[c.name]))
                for c in options:
                    cost = _parse_cost_cached(play_cost(c, modes.get(c.name)))
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
            if card.name in attacks or card.name in landfalls:
                in_play.append((t, card))
            src = mana_source(card, identity)
            if src is not None:
                if not src.sick:
                    units += [(len(bill), src.palette)] * src.amount
                sources.append(src)
            mode = modes.get(card.name)
            searches = [
                (w, hand)
                for w in fetches.get(card.name, ())
                # A transmute finds only the mana value it discarded.
                if not (mode and mode.transmute)
                or transmute_finds(card, next(c for c in g.library if c.name == w))
            ]
            # One loyalty activation a turn (CR 606.3), or the spell's one
            # search: the first card named that the library still holds and
            # the ability can find.
            for wanted in puts.get(card.name, ()):
                copies = [c for c in g.library if c.name == wanted]
                x = mode.x if mode else None
                if len(copies) > taken.count(wanted) and puts_onto_battlefield(card, copies[0], x):
                    searches.append((wanted, put))
                    break
            for wanted, into in searches:
                copies = [c for c in g.library if c.name == wanted]
                if len(copies) <= taken.count(wanted):
                    continue  # the library holds none: the search finds nothing
                taken.append(wanted)
                into.append(copies[0])
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
                        kept_lands=g.kept_lands,
                    )
            mill = mills.get(card.name)
            if mill is not None:
                mill_top(t, mill, milled)
        # Combat, after the line: a creature cast on an earlier turn is no
        # longer summoning-sick (CR 302.6), and attacks.
        for when, creature in in_play:
            if when < t and creature.name in attacks:
                mill_top(t, attacks[creature.name], milled)
        path.append(Turn(t, cast, units, bill, g, put, list(taken), milled))
    game._line_cache[key] = path
    return path


# --- A line whose land drop and discards the pilot declared ---------------
#
# Written from ADR 0017 §3, README "Discard" and HANDS.md hands 21 to 24.
#
# * The land drop is the pilot's list (README "The land drop"): each turn,
#   after the draw step and before any spell, play one land from hand, the
#   first entry of the list that one matches; a land no entry names is played
#   after every one that is named. ASSUMPTION (README): a tie inside one entry
#   goes to the card the decklist names first. The lands on the battlefield
#   then pay, every one but the one played this turn if it enters tapped - and
#   nothing is searched for: this is the one line the pilot played, not the
#   best of them. Two lands that make the same mana the same way, and that no
#   entry of either list tells apart, are the same card to the run (README),
#   so the tie goes to the kind of land the decklist names first.
# * A spell that draws takes the next cards off the top into your hand. A
#   spell among them is cast this turn if the line reaches it and the pool
#   still pays; a land among them waits for the next turn's drop, because this
#   turn's came before the line (ADR 0017).
# * Then the discard, from the whole hand: every card that has been drawn, or
#   put in hand, and has not been played, cast or discarded.
#   - A forced discard of n takes the cards the pilot's list names, the first
#     entry first, and then the cards it names nowhere. Where an entry holds
#     more than is left to take, which of them go is at random (README).
#   - "At random" (Desperate Ravings) is n cards picked uniformly from the
#     whole hand, whatever the list says.
#   - "Any number" of a kind (Borborygmos and Fblthp's lands) is every card of
#     that kind the list names.
# * Frantic Search: "Untap up to three lands." ASSUMPTION (README): read as
#   untapping the lands that paid for it, so what the turn has left to spend
#   is what it had before the spell.


@dataclass(frozen=True)
class Rummage:
    """What casting one card does to the hand."""

    draw: int
    discard: int = 0  # 0 with `any_number` for "any number"
    any_number: bool = False
    at_random: bool = False
    only: Callable[[Card], bool] | None = None
    untaps_its_cost: bool = False


@dataclass
class DeclaredTurn:
    number: int
    cast: list[Card]
    to_graveyard: list[Card]
    lands_in_play: int


class DeclaredPath(list):
    """The turns of one declared line, from turn 1."""

    def cast_by(self, name: str, turn: int) -> bool:
        return any(c.name == name for t in self[:turn] for c in t.cast)

    def in_graveyard_by(self, name: str, turn: int) -> bool:
        """Put into the graveyard by `turn`: a resolved instant or sorcery
        (CR 608.2n), a milled card, or a discarded one. Nothing takes a card
        back out."""
        return any(c.name == name for t in self[:turn] for c in t.to_graveyard)


# (library, land drop, discard list) -> land name -> where its kind is first named.
_LAND_RANKS: dict = {}


def declared_line_path(
    game: Game,
    line: Line,
    last_turn: int,
    land_drop: tuple[Callable[[Card], bool], ...],
    discard: tuple[Callable[[Card], bool], ...],
    fetches: dict[str, tuple[str, ...]],
    mills: dict[str, Mill],
    rummages: dict[str, Rummage],
    attacks: dict[str, Mill] | None = None,
    landfalls: dict[str, Mill] | None = None,
    returns: dict[str, Callable[[Card], bool]] | None = None,
) -> DeclaredPath:
    """Play `line` out through `last_turn` under a declared land drop and a
    declared discard list. `attacks` and `landfalls` are what a permanent the
    line cast mills each time it attacks or a land enters (see `line_path`).
    `returns` maps a card to the land cards that, once its mill is done, go
    from the whole graveyard onto the battlefield tapped (Lumra). A rock or a
    dork the line casts is a mana source (ADR 0018). Cached on the game."""
    attacks = attacks or {}
    landfalls = landfalls or {}
    returns = returns or {}
    key = ("declared", line, last_turn, land_drop, discard,
           tuple(sorted(fetches.items())), tuple(sorted(mills.items())),
           tuple(sorted(rummages.items())), tuple(sorted(attacks.items())),
           tuple(sorted(landfalls.items())), tuple(sorted(returns.items())))  # fmt: skip
    if key in game._line_cache:
        return game._line_cache[key]
    # The card picked at random is a function of the deal, and the same one
    # every time this game is asked: a string seed is hashed stably.
    rng = random.Random("|".join(c.name for c in game.cards))
    named = {n for entry in line for n in entry}
    order = {n: j for entry in line for j, n in enumerate(entry)}
    # Where the decklist first names each kind of land: two lands that make
    # the same mana the same way, and that no entry of either list tells
    # apart, are one card to this run, so a tie between them is no tie.
    def kind(c: Card) -> tuple:
        return (c.produces, c.enters_tapped, c.makes_mana, c.lasts,
                tuple(wants(c) for wants in land_drop + discard))  # fmt: skip

    ranks_key = (id(game.library), land_drop, discard)
    if ranks_key not in _LAND_RANKS:
        first_named: dict[tuple, int] = {}
        for i, c in enumerate(game.library):
            first_named.setdefault(kind(c), i)
        _LAND_RANKS[ranks_key] = {c.name: first_named[kind(c)] for c in game.library}
    rank = _LAND_RANKS[ranks_key]
    library = list(game.cards)  # the top of the shuffled library
    top = 0
    taken: list[str] = []
    # The command zone is not the hand: a discard never takes the commander.
    command: list[Card] = [c for c in game.commanders if c.name in named]
    hand: list[Card] = []
    in_play: list[tuple[Card, int]] = []
    # Permanents the line cast that trigger, and the turn each was cast on.
    triggers: list[tuple[Card, int]] = []
    identity = frozenset().union(*(c.identity for c in game.commanders))
    # Mana sources the line cast, and the turn each was cast on.
    sources: list[tuple[Source, int]] = []
    # Every card in the graveyard now: a returned land leaves it.
    graveyard: list[Card] = []
    path = DeclaredPath()

    def draw(n: int) -> list[Card]:
        nonlocal top
        if top + n > len(library) and len(game.cards) < game.library_size:
            raise ValueError("the deal is shallower than this line reads: raise its depth")
        cards = library[top : top + n]
        top += len(cards)
        taken.extend(c.name for c in cards)
        return cards

    def tier_of(c: Card, tiers) -> int:
        return next((i for i, wants in enumerate(tiers) if wants(c)), len(tiers))

    hand += draw(7)
    for t in range(1, last_turn + 1):
        if game.on_the_draw or t > 1:
            hand += draw(1)
        # The land drop.
        lands = [c for c in hand if c.playable_land]
        if lands:
            land = min(lands, key=lambda c: (tier_of(c, land_drop), rank.get(c.name, 0)))
            hand.remove(land)
            in_play.append((land, t))
        to_graveyard: list[Card] = []
        returned_now: list[Card] = []  # of this turn's, the lands a card returned

        def mill_off(mill: Mill) -> None:
            milled = draw(mill.cards)
            kept = mill.kept(milled, game.library)
            for i, c in enumerate(milled):
                (hand if i in kept else to_graveyard).append(c)

        def landfall(entered: int) -> None:
            """Each land that entered fires every landfall permanent the
            line has cast so far: one on an earlier turn sees the drop, and
            any one on the battlefield sees a land a spell returns."""
            for permanent, _ in list(triggers):
                if permanent.name in landfalls:
                    for _ in range(entered):
                        mill_off(landfalls[permanent.name])

        # The drop came before the line, so only a permanent cast on an
        # earlier turn is there to see it.
        if lands:
            for permanent, when in list(triggers):
                if when < t and permanent.name in landfalls:
                    mill_off(landfalls[permanent.name])
        # Only lands that took a drop pay (README "returns": a returned land
        # took none, and a turn's bill is held to its drops).
        pool = [
            (0, c.produces)
            for c, played in in_play
            if played and c.makes_mana and not (played == t and c.enters_tapped)
        ]
        # A source cast on an earlier turn adds from the start of this one; a
        # creature cast this turn is summoning-sick (CR 302.6).
        pool += [(0, src.palette) for src, when in sources if when < t for _ in range(src.amount)]
        bill: list[Cost] = []
        cast: list[Card] = []
        while True:
            chosen = None
            for entry in line:
                options = [c for c in hand + command if c.name in entry]
                options.sort(key=lambda c: (_mana_value(c), order[c.name]))
                for c in options:
                    cost = _parse_cost_cached(c.mana_cost)
                    if _settles(pool, bill + [cost]):
                        chosen = (c, cost)
                        break
                if chosen:
                    break
            if not chosen:
                break
            card, cost = chosen
            (command if card in command else hand).remove(card)
            cast.append(card)
            if card.name in attacks or card.name in landfalls:
                triggers.append((card, t))
            rummage = rummages.get(card.name)
            if not (rummage and rummage.untaps_its_cost):
                bill.append(cost)
            # A rock pays for what is cast after it this turn and never for
            # itself; a dork only from the next turn (ADR 0018).
            src = mana_source(card, identity)
            if src is not None:
                sources.append((src, t))
                if not src.sick:
                    pool += [(len(bill), src.palette)] * src.amount
            if any(k in card.type_line for k in ("Instant", "Sorcery")):
                to_graveyard.append(card)
            for wanted in fetches.get(card.name, ()):
                copies = [c for c in game.library if c.name == wanted]
                if len(copies) <= taken.count(wanted):
                    continue
                taken.append(wanted)
                hand.append(copies[0])
                below = next(
                    (i for i in range(top, len(library)) if library[i].name == wanted), None
                )
                if below is not None:
                    del library[below]
            mill = mills.get(card.name)
            if mill is not None:
                milled = draw(mill.cards)
                kept = mill.kept(milled, game.library)
                for i, c in enumerate(milled):
                    (hand if i in kept else to_graveyard).append(c)
            back = returns.get(card.name)
            if back is not None:
                # "Then return all land cards from your graveyard to the
                # battlefield tapped": this turn's and every earlier one's.
                gone = [c for c in graveyard if back(c)]
                graveyard = [c for c in graveyard if not back(c)]
                this_turn = list(to_graveyard)
                for c in returned_now:
                    this_turn.remove(c)
                gone += [c for c in this_turn if back(c)]
                returned_now += [c for c in this_turn if back(c)]
                in_play += [(c, 0) for c in gone]  # on the battlefield, by no drop
                landfall(len(gone))
            if rummage is not None:
                hand += draw(rummage.draw)
                allowed = [c for c in hand if rummage.only is None or rummage.only(c)]
                if rummage.at_random:
                    gone = rng.sample(allowed, min(rummage.discard, len(allowed)))
                elif rummage.any_number:
                    gone = [c for c in allowed if tier_of(c, discard) < len(discard)]
                else:
                    gone, left = [], rummage.discard
                    for tier in range(len(discard) + 1):
                        these = [c for c in allowed if tier_of(c, discard) == tier]
                        if len(these) > left:
                            these = rng.sample(these, left)
                        gone += these
                        left -= len(these)
                        if left == 0:
                            break
                for c in gone:
                    hand.remove(c)
                    to_graveyard.append(c)
        # Combat, after the line: a creature cast on an earlier turn attacks.
        for creature, when in triggers:
            if when < t and creature.name in attacks:
                mill_off(attacks[creature.name])
        # Put into the graveyard this turn, whatever left it again since.
        path.append(DeclaredTurn(t, cast, to_graveyard, len(in_play)))
        rest = list(to_graveyard)
        for c in returned_now:
            rest.remove(c)
        graveyard += rest
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


# --- The line with a declared land drop ---------------------------------------
#
# `line_path` pays from the gate's lands: whichever lands would pay, asked
# afresh each turn, which is the reading a file takes when it declares no
# `[land_drop]`. A file that declares one plays the lands its list chooses,
# one a turn, and the line pays from those and nothing else. That is what
# `drop_line_path` plays. Written from README ("One land drop, one declared
# policy", "Delayed effects: Urza's Saga", "An activation the line pays for:
# Expedition Map"), ADR 0019 and the Comprehensive Rules, and held to HANDS.md
# hands 12, 17 and 42 by checker/test_land_drop.py:
#
# * The drop (CR 305.2: one land a turn, from the hand). The list is read in
#   order and the first entry a land in hand matches is played; a land no entry
#   names is played after every land one does, never not at all. A tie inside
#   an entry goes to the deeper look, then to the card the decklist names
#   first (README). ASSUMPTION (README "Effects"): the standard library gives a
#   land tagged `surveil` or `scry` a look of one when it is played. A look
#   digs deeper only where the file routes what it sees (`to_graveyard`):
#   otherwise every card stays on top, the look moves nothing, and it breaks
#   no tie, so Hedge Maze in decks/lantern.txt, whose surveil no file routes,
#   is played in decklist order like any other land. (Measured against the
#   engine on small decks rather than read from it: the README's "the deeper
#   look" is the routed case, HANDS.md hand 12.) A land playable as a drop is
#   the gate's (`playable_land`): a modal double-faced card's land face counts,
#   Search for Azcanta does not.
# * What a land pays: every land in play untaps, so a land played on an earlier
#   turn pays once a turn; one played this turn pays this turn unless it enters
#   tapped. What it pays is the gate's reading of it (`produces`, `makes_mana`,
#   `lasts`): a fetchland pays the untapped lands it finds, Maze of Ith nothing,
#   the Saga for the turn it lands and the two after.
# * A delayed chapter: a Saga gets a lore counter as it enters and after each
#   draw step (CR 714.2b, 714.3b), so chapter III resolves two turns after the
#   drop, after that turn's draw and before its land drop; it searches the
#   library for the first card it names that the library still holds and puts
#   it onto the battlefield, and the Saga is sacrificed (CR 714.4). The Saga's
#   {C} that turn is still that turn's mana (HANDS.md hand 17).
# * An activation (CR 602.2b, 118.3): the cost before the colon is paid in
#   full, a sacrifice in it takes the permanent out of play, and an artifact
#   has no summoning sickness to wait out, so a copy the line cast this turn may
#   be activated this turn. ASSUMPTION (README, ADR 0019): the line's entry that
#   names the card first activates a copy in play, then casts a copy from hand,
#   and does either whenever the pool pays, even with nothing left to find. It
#   is paid after the drop, except where what it would fetch is a land the drop
#   list ranks above every land in hand: then, if the library still holds that
#   land, it is paid before the drop out of what is already in play, and the
#   drop plays the land it found. The drop's own mana then pays only for what
#   comes after.
# * A tutor to hand is read from the card: "search your library for an
#   artifact card [with mana value N or less], reveal it, put it into your
#   hand" on a cast (Fabricate), an enters trigger (Trinket Mage) or a loyalty
#   ability the walker's starting loyalty pays for (Tezzeret, Cruel Captain's
#   −3 off four, CR 606.3-606.6, the same reading as the Seeker's). The card it
#   found is in hand, so the line may cast it this turn; a land a tutor puts in
#   hand waits for the next turn's drop (ADR 0017).
# * Everything else is `line_path`'s: the first entry the pool pays for is
#   cast, the line is read again from its top after every cast or activation,
#   a rock pays only for what comes after it, a transmute and a chosen X are
#   what the pilot pays, the commander is in hand from turn 1.


_TO_HAND = re.compile(
    r"^(?:[−-](\d+): )?(?:When [^,]*, )?(?:you may )?[Ss]earch your library for an? (\w+) card"
    r"(?: with mana value (\d+) or less)?, reveal (?:it|that card), put it into your hand",
    re.M,
)


def searches_to_hand(source: Card, target: Card) -> bool:
    """Could `source`, cast, put `target` from the library into your hand that
    turn: by resolving, by an enters trigger, or by a loyalty ability its
    starting loyalty pays for?"""
    for m in _TO_HAND.finditer(source.oracle):
        minus, kind, most = m.group(1), m.group(2), m.group(3)
        if minus is not None and (source.loyalty is None or int(minus) > source.loyalty):
            continue
        if kind.capitalize() not in target.type_line:
            continue
        if most is not None and _mana_value(target) > int(most):
            continue
        return True
    return False


_CHAPTER_III_ONTO_BATTLEFIELD = re.compile(
    r"^III — Search your library for an? (\w+) card with mana cost \{0\} or \{1\}, put it onto "
    r"the battlefield",
    re.M,
)


def chapter_three_puts(saga: Card, target: Card) -> bool:
    """Could `saga`'s third chapter put `target` onto the battlefield? Its mana
    cost must be {0} or {1}."""
    m = _CHAPTER_III_ONTO_BATTLEFIELD.search(saga.oracle)
    return (
        bool(m)
        and m.group(1).capitalize() in target.type_line
        and target.mana_cost in ("{0}", "{1}")
    )


_TAP_SACRIFICE_SEARCH = re.compile(
    r"^((?:\{\d+\})+), \{T\}, Sacrifice this artifact: Search your library for an? (\w+) card, "
    r"reveal it, put it into your hand",
    re.M,
)


def activated_search(card: Card) -> tuple[str, str] | None:
    """(cost, card type it finds) of an artifact's "[cost], {T}, Sacrifice
    this artifact: Search your library for a ... card, reveal it, put it into
    your hand", or None. Everything before the colon is the cost (CR 602.1a)."""
    m = _TAP_SACRIFICE_SEARCH.search(card.oracle)
    return (m.group(1), m.group(2).lower()) if m else None


def _looks(card: Card) -> int:
    """How deep a land looks when it is played: the standard library's surveil
    and scry lands look one."""
    return 1 if card.tags & {"surveil", "scry"} else 0


@dataclass(frozen=True)
class LandDrop:
    """A declared `[land_drop] prefer`, one predicate over a land per entry,
    and whether the file routes what a land's look sees (only then is one
    look deeper than another)."""

    prefer: tuple[Callable[[Card], bool], ...]
    routed: bool = False

    def rank(self, card: Card) -> int:
        return next((i for i, wants in enumerate(self.prefer) if wants(card)), len(self.prefer))

    def choose(self, lands: list[Card], decklist: list[Card]) -> Card:
        """The land played from `lands`: the best-ranked entry, then the
        deeper look, then the card the decklist names first."""
        position = {c.name: i for i, c in reversed(list(enumerate(decklist)))}
        return min(
            lands,
            key=lambda c: (
                self.rank(c),
                -_looks(c) if self.routed else 0,
                position.get(c.name, len(decklist)),
            ),
        )


def drop_line_path(
    game: Game,
    line: Line,
    last_turn: int,
    land_drop: LandDrop,
    fetches: dict[str, tuple[str, ...]] | None = None,
    puts: dict[str, tuple[str, ...]] | None = None,
    modes: dict[str, Mode] | None = None,
    chapters: dict[str, tuple[str, ...]] | None = None,
) -> LinePath:
    """Play `line` out through `last_turn` with the lands `land_drop` plays.

    `fetches` maps a card to what it puts into your hand when cast, `puts` to
    what it puts onto the battlefield (both the first of them the library still
    holds and the card can find), `modes` to how the pilot plays it, and
    `chapters` maps a Saga land to what its third chapter puts onto the
    battlefield. An artifact with a tap-and-sacrifice search is activated for
    what `fetches` names for it, and its cast fetches nothing. Cached on the
    game."""
    fetches = fetches or {}
    puts = puts or {}
    modes = modes or {}
    chapters = chapters or {}
    key = (
        "drop-path",
        line,
        last_turn,
        land_drop,
        tuple(sorted(fetches.items())),
        tuple(sorted(puts.items())),
        tuple(sorted(modes.items(), key=lambda kv: kv[0])),
        tuple(sorted(chapters.items())),
    )
    if key in game._line_cache:
        return game._line_cache[key]
    named = {n for entry in line for n in entry}
    identity = frozenset().union(*(c.identity for c in game.commanders))
    order = {n: j for entry in line for j, n in enumerate(entry)}
    g = game
    hand: list[Card] = [c for c in game.commanders if c.name in named]
    lands: list[Card] = []  # playable lands in hand
    in_play: list[tuple[int, Card]] = []  # (turn played, land)
    sources: list[Source] = []  # what the line cast that makes mana
    ready: list[Card] = []  # permanents the line cast with an activation, still in play
    waiting: list[tuple[int, Card]] = []  # (turn its chapter III resolves, the Saga)
    taken: list[str] = []  # names seen or taken out of the library
    seen_so_far = 0
    path = LinePath()

    def holds(name: str) -> bool:
        return sum(1 for c in g.library if c.name == name) > taken.count(name)

    def take(name: str) -> Card:
        """Take one copy of `name` out of the library; later draws move up."""
        nonlocal g
        taken.append(name)
        below = next(
            (i for i in range(seen_so_far, len(g.cards)) if g.cards[i].name == name), None
        )
        if below is not None:
            g = Game(
                g.cards[:below] + g.cards[below + 1 :],
                g.on_the_draw,
                g.library_size - 1,
                commanders=g.commanders,
                library=g.library,
            )
        return next(c for c in g.library if c.name == name)

    def pays(played: int, land: Card, t: int) -> bool:
        if not land.makes_mana:
            return False
        if land.lasts is not None and t - played >= land.lasts:
            return False  # sacrificed by its last chapter
        return not (land.enters_tapped and played == t)

    def activation_finds(card: Card) -> str | None:
        """What activating `card` would fetch: the first card `fetches` names
        for it, of the type its text finds, that the library still holds."""
        _, kind = activated_search(card)
        for wanted in fetches.get(card.name, ()):
            copy = next(c for c in g.library if c.name == wanted)
            if kind.capitalize() in copy.type_line and holds(wanted):
                return wanted
        return None

    def activation_cost(card: Card) -> Cost:
        return _parse_cost_cached(activated_search(card)[0])

    for t in range(1, last_turn + 1):
        new = g.seen(t)[seen_so_far:]
        seen_so_far = g.seen_count(t)
        taken += [c.name for c in new]
        hand += [c for c in new if c.name in named]
        lands += [c for c in new if c.playable_land]
        put: list[Card] = []
        cast: list[Card] = []
        # Chapter III, after the draw step and before the drop.
        for _, saga in [w for w in waiting if w[0] == t]:
            for wanted in chapters.get(saga.name, ()):
                target = next(c for c in g.library if c.name == wanted)
                if holds(wanted) and chapter_three_puts(saga, target):
                    put.append(take(wanted))
                    break
        waiting = [w for w in waiting if w[0] != t]
        units: list[Unit] = [(0, land.produces) for p, land in in_play if pays(p, land, t)]
        units += [(0, s.palette) for s in sources for _ in range(s.amount)]
        bill: list[Cost] = []
        # The one payment before the drop: an activation for a land the drop
        # ranks above every land in hand, out of what is already in play.
        for permanent in list(ready):
            wanted = activation_finds(permanent)
            if wanted is None:
                continue
            target = next(c for c in g.library if c.name == wanted)
            if not target.playable_land or any(
                land_drop.rank(c) <= land_drop.rank(target) for c in lands
            ):
                continue
            if _settles(units, bill + [activation_cost(permanent)]):
                bill.append(activation_cost(permanent))
                ready.remove(permanent)
                lands.append(take(wanted))
        # The drop.
        if lands:
            land = land_drop.choose(lands, g.library)
            lands.remove(land)
            in_play.append((t, land))
            if pays(t, land, t):
                units.append((len(bill), land.produces))
            if land.name in chapters:
                waiting.append((t + 2, land))
        # The line.
        while True:
            chosen = None
            for entry in line:
                for permanent in ready:
                    if permanent.name in entry and _settles(
                        units, bill + [activation_cost(permanent)]
                    ):
                        chosen = ("activate", permanent, activation_cost(permanent))
                        break
                if chosen:
                    break
                options = [c for c in hand if c.name in entry]
                options.sort(key=lambda c: (_paid(c, modes), order[c.name]))
                for c in options:
                    cost = _parse_cost_cached(play_cost(c, modes.get(c.name)))
                    if _settles(units, bill + [cost]):
                        chosen = ("cast", c, cost)
                        break
                if chosen:
                    break
            if not chosen:
                break
            how, card, cost = chosen
            bill.append(cost)
            if how == "activate":
                # Sacrificed as part of the cost: at most once, ever.
                ready.remove(card)
                wanted = activation_finds(card)
                if wanted is not None:
                    found = take(wanted)
                    if found.playable_land:
                        lands.append(found)  # waits for the next turn's drop
                    else:
                        hand.append(found)
                continue
            cast.append(card)
            hand.remove(card)
            src = mana_source(card, identity)
            if src is not None:
                if not src.sick:
                    units += [(len(bill), src.palette)] * src.amount
                sources.append(src)
            if activated_search(card) is not None:
                if card.name in fetches:
                    ready.append(card)
                continue
            mode = modes.get(card.name)
            for wanted in fetches.get(card.name, ()):
                if not holds(wanted):
                    continue
                target = next(c for c in g.library if c.name == wanted)
                finds = (
                    transmute_finds(card, target)
                    if mode and mode.transmute
                    else searches_to_hand(card, target)
                )
                if finds:
                    found = take(wanted)
                    (lands if found.playable_land else hand).append(found)
                    break
            for wanted in puts.get(card.name, ()):
                if not holds(wanted):
                    continue
                target = next(c for c in g.library if c.name == wanted)
                if puts_onto_battlefield(card, target, mode.x if mode else None):
                    put.append(take(wanted))
                    break
        path.append(Turn(t, cast, units, bill, g, put, list(taken)))
    game._line_cache[key] = path
    return path


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
    # At most this many games, where fewer than compare.py's --games will do:
    # a question played through `line_path` costs far more per game than a
    # count, and one the engine answers by sampling carries an error bar of
    # its own that a longer run cannot shrink.
    games: int | None = None


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
RUMBLE, TILLING = "Malevolent Rumble", "Midnight Tilling"
ANALYST, WRENN = "Aftermath Analyst", "Wrenn and Seven"
SIX, EXPLORER, LUMRA = "Six", "Icetill Explorer", "Lumra, Bellow of the Woods"
# The Loam north star, as decks/loam.criteria.toml asks it (#101): Life from
# the Loam put into the graveyard AND Borborygmos and Fblthp cast, by turn N,
# on one line and one pool.
#
# [[effect]] match = 'name:"Spellseeker"', on = "cast",
#            fetch = ['name:"Life from the Loam"'], to = "hand"
# [[effect]] match = 'name:"Malevolent Rumble"' and 'name:"Midnight Tilling"',
#            to_hand = ['name:"Spellseeker"', 't:land']
# [[effect]] match = 'name:"Six" t:treefolk', on = "attack", to_hand = ['t:land']
# [[effect]] match = 'name:"Lumra, Bellow of the Woods"', on = "cast", mill = 4
#            (and no `returns`: see Lumra below)
# [casting] prefer = NORTH_STAR_LINE, below, one entry per tuple
# [discard] prefer = [Loam, a nonland card the line does not name]
# [land_drop] prefer = [lands entering tapped by the pilot's choice, taplands,
#                       fetchlands, lands making none of {G}{U}{R}, nonbasic
#                       lands, any land]
#
# Spellseeker's enters trigger searches the library for an instant or sorcery
# with mana value 2 or less and puts it into your hand; Loam is a sorcery at
# mana value 2, and the only one the effect names. The line is read again
# after the fetch (HANDS.md hand 36), and a Loam cast resolves into the
# graveyard (CR 608.2n). The other way in is a mill, read off each card:
#
# * Aftermath Analyst: "When this creature enters, mill three cards."
# * Malevolent Rumble: "Reveal the top four cards of your library. You may put
#   a permanent card from among them into your hand. Put the rest into your
#   graveyard." The pilot keeps Spellseeker, which finds a Loam still in the
#   library, and otherwise a land.
# * Midnight Tilling: "Mill four cards, then you may return a permanent card
#   from among them to your hand." The same choice.
# * Wrenn and Seven, +1, the turn it is cast: "Reveal the top four cards of
#   your library. Put all land cards revealed this way into your hand and the
#   rest into your graveyard." ASSUMPTION (the standard library's entry): it is
#   activated once, on the turn it is cast, and not on the turns after.
#
# Attack and landfall (#89), read off each card and the rules:
#
# * Six: "Whenever Six attacks, mill three cards. You may put a land card from
#   among them into your hand." A creature cannot attack the turn it came under
#   your control (CR 302.6), so Six first attacks the turn after it is cast.
#   Combat follows the main phase the line is cast in (CR 505, 506), so the
#   mill comes after that turn's spells. The pilot keeps a land. ASSUMPTION
#   (README): it attacks every turn it can and nobody blocks or removes it -
#   there is no opponent at this table.
# * Icetill Explorer: "Whenever a land you control enters, mill a card." The
#   land drop comes before the line, so the drop of the turn it is cast does
#   not trigger it; every drop after that does. ASSUMPTION (README): its
#   additional land a turn, and playing lands from the graveyard, are not used.
# * Lumra: "When Lumra enters, mill four cards. Then return all land cards
#   from your graveyard to the battlefield tapped." The file declares the mill
#   and not the return (its header says why), so the lands stay where they are.
#
# And the third way in is a discard, read off each card:
#
# * Frantic Search: "Draw two cards, then discard two cards. Untap up to three
#   lands." The untap is read as the lands that paid for it (README).
# * Izzet Charm, its third mode: "Draw two cards, then discard two cards."
#   ASSUMPTION (the standard library's entry): a line casting it chooses that
#   mode.
# * Desperate Ravings: "Draw two cards, then discard a card at random."
# * Borborygmos and Fblthp: "When Borborygmos and Fblthp enters, draw a card.
#   Then you may discard any number of land cards." The list names no land, so
#   it discards none (README "Discard": "any number" is every eligible card
#   the list names).
#
# The commander is cast from the command zone out of the same pool, and is
# never in hand to be discarded (README "Mana, as a budget"). Birds of
# Paradise and Elvish Mystic are sources from the turn after the line casts
# them (ADR 0018, CR 302.6).
#
# Dredge is not a route here (ADR 0017). Everything else is
# `declared_line_path`: the file declares its land drop and its discard list,
# so this plays the line the pilot declared rather than the best one.
FRANTIC, CHARM, RAVINGS = "Frantic Search", "Izzet Charm", "Desperate Ravings"
BIRDS, MYSTIC = "Birds of Paradise", "Elvish Mystic"
BORBORYGMOS = "Borborygmos and Fblthp"
NORTH_STAR_LINE = (
    (BORBORYGMOS,), (LOAM,), (BIRDS, MYSTIC), (FRANTIC,), (SEEKER,),
    (RUMBLE,), (TILLING,), (CHARM,), (RAVINGS,), (ANALYST,), (WRENN,), (SIX,),
    (EXPLORER,), (LUMRA,),
)  # fmt: skip
SEEKER_FETCHES = {SEEKER: (LOAM,)}
_PERMANENT_TYPES = ("Artifact", "Creature", "Enchantment", "Land", "Planeswalker", "Battle")


def _is_permanent_card(c: Card) -> bool:
    """A permanent card, by the face it has in the library (CR 110.4)."""
    return any(t in c.type_line.split("//")[0] for t in _PERMANENT_TYPES)


def _is_land(c: Card) -> bool:
    return "Land" in c.type_line.split("//")[0]


def _is_seeker(c: Card) -> bool:
    return c.name == SEEKER


_PILOT_KEEPS = (_is_seeker, _is_land)
LOAM_MILLS = {
    ANALYST: Mill(3),
    RUMBLE: Mill(4, keep_up_to=1, keep_only=_is_permanent_card, prefer=_PILOT_KEEPS),
    TILLING: Mill(4, keep_up_to=1, keep_only=_is_permanent_card, prefer=_PILOT_KEEPS),
    WRENN: Mill(4, keep_every=_is_land),
}
LOAM_RUMMAGES = {
    FRANTIC: Rummage(draw=2, discard=2, untaps_its_cost=True),
    CHARM: Rummage(draw=2, discard=2),
    RAVINGS: Rummage(draw=2, discard=1, at_random=True),
}


def _is_tapland(c: Card) -> bool:
    return "tapland" in c.tags


def _is_loam(c: Card) -> bool:
    return c.name == LOAM


LOAM_ATTACKS = {SIX: Mill(3, keep_up_to=1, keep_only=_is_land, prefer=(_is_land,))}
LOAM_LANDFALLS = {EXPLORER: Mill(1)}

# The north star's own choices.
NORTH_STAR_MILLS = {**LOAM_MILLS, LUMRA: Mill(4)}
NORTH_STAR_RUMMAGES = {
    **LOAM_RUMMAGES,
    BORBORYGMOS: Rummage(draw=1, any_number=True, only=_is_land),
}
_IN_THE_LINE = frozenset(n for entry in NORTH_STAR_LINE for n in entry)


def _is_offline(c: Card) -> bool:
    """A nonland card the line never casts: the file's '-t:land -name:...'."""
    return not _is_land(c) and c.name not in _IN_THE_LINE


def _enters_tapped_by_choice(c: Card) -> bool:
    return _is_land(c) and "conditional-tapland" in c.tags


def _is_fetchland(c: Card) -> bool:
    return _is_land(c) and "fetchland" in c.tags


def _makes_none_of_the_colours(c: Card) -> bool:
    # 't:land -produces:g -produces:u -produces:r', off Scryfall's own list
    return _is_land(c) and not (c.listed & {"G", "U", "R"})


def _is_nonbasic(c: Card) -> bool:
    return _is_land(c) and "Basic" not in c.type_line


NORTH_STAR_LAND_DROP = (
    _enters_tapped_by_choice,
    _is_tapland,
    _is_fetchland,
    _makes_none_of_the_colours,
    _is_nonbasic,
    _is_land,
)
NORTH_STAR_DISCARDS = (_is_loam, _is_offline)
NORTH_STAR_TURNS = 7
# Deep enough for turn 7 however the line fires: seven draw steps, the fetch,
# every mill and rummage once, Six's attacks on turns 3 to 7 at the most and
# a landfall for each drop after the Explorer, cast on turn 3 at the earliest.
# `declared_line_path` refuses to run short rather than answer from fewer.
NORTH_STAR_DEPTH = 7 + 1 + (3 + 4 + 4 + 4 + 4) + (2 + 2 + 2 + 1) + 3 * 5 + 4


def _north_star_path(g: Game) -> DeclaredPath:
    return declared_line_path(
        g,
        NORTH_STAR_LINE,
        NORTH_STAR_TURNS,
        NORTH_STAR_LAND_DROP,
        NORTH_STAR_DISCARDS,
        SEEKER_FETCHES,
        NORTH_STAR_MILLS,
        NORTH_STAR_RUMMAGES,
        attacks=LOAM_ATTACKS,
        landfalls=LOAM_LANDFALLS,
    )


def _north_star(turn: int) -> Callable[[Game], bool]:
    # { turn = N, query = 'name:"Life from the Loam"', zone = "graveyard", min = 1 }
    # { turn = N, cast = 'name:"Borborygmos and Fblthp"', min = 1 }
    def ask(g: Game) -> bool:
        path = _north_star_path(g)
        return path.in_graveyard_by(LOAM, turn) and path.cast_by(BORBORYGMOS, turn)

    return ask


def _north_star_loam(turn: int) -> Callable[[Game], bool]:
    # { turn = N, query = 'name:"Life from the Loam"', zone = "graveyard", min = 1 }
    return lambda g: _north_star_path(g).in_graveyard_by(LOAM, turn)


def _north_star_commander(turn: int) -> Callable[[Game], bool]:
    # { turn = N, cast = 'name:"Borborygmos and Fblthp"', min = 1 }
    return lambda g: _north_star_path(g).cast_by(BORBORYGMOS, turn)


# [casting] prefer = ['name:"Aftermath Analyst"', 'name:"Life from the Loam"']
#
# The Analyst alone, as loam-analyst.criteria.toml asks it: "When this creature
# enters, mill three cards", and then Loam, cast off whatever is left.
ANALYST_LINE = ((ANALYST,), (LOAM,))
ANALYST_MILLS = {ANALYST: Mill(3)}


def _analyst_route_loam_in_graveyard_by_5(g: Game) -> bool:
    # { turn = 5, query = 'name:"Life from the Loam"', zone = "graveyard", min = 1 }
    return line_path(g, ANALYST_LINE, 5, mills=ANALYST_MILLS).in_graveyard_by(LOAM, 5)


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


LANTERN, TRINKET = "Lantern of Insight", "Trinket Mage"
# [[effect]] match = 'name:"Trinket Mage"', on = "cast",
#            fetch = ['name:"Lantern of Insight"'], to = "hand"
# [casting] prefer = ['name:"Trinket Mage"', 'name:"Lantern of Insight"']
#
# Trinket Mage's enters trigger searches for an artifact with mana value 1 or
# less and puts it into your hand; the Lantern is the one the effect names.
ROUTE_B_LINE: Line = ((TRINKET,), (LANTERN,))
TRINKET_FETCHES = {TRINKET: (LANTERN,)}


def _lantern_on_the_battlefield_by_5(g: Game) -> bool:
    # { turn = 5, query = 'name:"Lantern of Insight"', zone = "battlefield", min = 1 }
    #
    # An artifact is not a land, so no land drop puts it there: it is on the
    # battlefield once it has been cast and has resolved (CR 608.3), and a
    # Lantern still in hand is in hand (README "[casting]": a permanent the
    # line cast is counted on the battlefield). Nothing in this file puts one
    # there any other way, and nothing here takes it off again.
    return line_path(g, ROUTE_B_LINE, 5, TRINKET_FETCHES).cast_by(LANTERN, 5)


TEZZERET = "Tezzeret the Seeker"
# [[effect]] match = 'name:"Tezzeret the Seeker"', on = "cast",
#            fetch = ['name:"Lantern of Insight"'], to = "battlefield"
# [casting] prefer = ['name:"Lantern of Insight"', 'name:"Tezzeret the Seeker"']
#
# The Lantern first, cast for {1} when it is in hand; then the Seeker, whose
# −1 puts it onto the battlefield from the library the turn he resolves (the
# line's notes, and HANDS.md hand 40). Everything else is `line_path`.
SEEKER_ROUTE_LINE: Line = ((LANTERN,), (TEZZERET,))
TEZZERET_PUTS = {TEZZERET: (LANTERN,)}


def _seeker_route_lantern_on_the_battlefield_by_5(g: Game) -> bool:
    # { turn = 5, query = 'name:"Lantern of Insight"', zone = "battlefield", min = 1 }
    return line_path(g, SEEKER_ROUTE_LINE, 5, puts=TEZZERET_PUTS).on_battlefield_by(LANTERN, 5)


def _seeker_cast_by_5(g: Game) -> bool:
    # { turn = 5, cast = 'name:"Tezzeret the Seeker"', min = 1 }
    return line_path(g, SEEKER_ROUTE_LINE, 5, puts=TEZZERET_PUTS).cast_by(TEZZERET, 5)


DIZZY, WHIR = "Dizzy Spell", "Whir of Invention"
# [[effect]] match = 'name:"Whir of Invention"', on = "cast", cost = "{1}{U}{U}{U}",
#            fetch = ['name:"Lantern of Insight"'], to = "battlefield"
# [[effect]] match = 'name:"Tezzeret the Seeker"', on = "cast",
#            fetch = ['name:"Lantern of Insight"'], to = "battlefield"
# [[effect]] match = 'name:"Dizzy Spell"', on = "cast", cost = "{1}{U}{U}",
#            fetch = ['name:"Lantern of Insight"'], to = "hand"
# [casting] prefer = [Lantern, Whir of Invention, Tezzeret the Seeker, Dizzy Spell]
#
# Whir at X = 1, Dizzy Spell by its transmute: the line's notes say how each
# is read from the card. The Seeker is SEEKER_ROUTE_LINE's.
TUTORS_ROUTE_LINE: Line = ((LANTERN,), (WHIR,), (TEZZERET,), (DIZZY,))
TUTORS_ROUTE = dict(
    fetches={DIZZY: (LANTERN,)},
    puts={WHIR: (LANTERN,), TEZZERET: (LANTERN,)},
    modes={DIZZY: TRANSMUTE, WHIR: x_is(1)},
)


def _tutors_route(holds: Callable[[LinePath], bool]) -> Callable[[Game], bool]:
    return lambda g: holds(line_path(g, TUTORS_ROUTE_LINE, 5, **TUTORS_ROUTE))


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
        "loam-access.criteria.toml",
        "Loam castable by turn 5, so Loam in the graveyard by turn 5",
        _loam_castable_by_5,
        5,
    ),
    Question(
        "loam",
        "loam-analyst.criteria.toml",
        "Life from the Loam in the graveyard by turn 5, cast or milled by the Analyst",
        _analyst_route_loam_in_graveyard_by_5,
        5 + 3,  # turn 5, and the three cards the Analyst mills
    ),
    Question(
        "loam",
        "loam-access.criteria.toml",
        "control: {1}{G} payable by turn 5, no Loam asked",
        _one_green_by_5,
        5,
    ),
    Question(
        "loam",
        "loam-access.criteria.toml",
        "a two-mana Loam Access card and {1}{G} for it, turn 3",
        _loam_two_drop_and_mana_t3,
        3,
    ),
    Question(
        "loam",
        "loam-access.criteria.toml",
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
        "lantern-route-b.criteria.toml",
        "Lantern of Insight on the battlefield by turn 5",
        _lantern_on_the_battlefield_by_5,
        6,  # turn 5, and one card deeper for the Lantern a fetch takes out
    ),
    Question(
        "lantern",
        "lantern-route-seeker.criteria.toml",
        "Lantern of Insight on the battlefield by turn 5, cast or put there by the Seeker",
        _seeker_route_lantern_on_the_battlefield_by_5,
        6,  # turn 5, and one card deeper for the Lantern the Seeker takes out
    ),
    Question(
        "lantern",
        "lantern-route-seeker.criteria.toml",
        "Tezzeret the Seeker cast by turn 5",
        _seeker_cast_by_5,
        6,
    ),
    Question(
        "lantern",
        "lantern-route-tutors.criteria.toml",
        "Lantern of Insight on the battlefield by turn 5, cast or put there by a tutor",
        # { turn = 5, query = 'name:"Lantern of Insight"', zone = "battlefield", min = 1 }
        _tutors_route(lambda p: p.on_battlefield_by(LANTERN, 5)),
        6,  # turn 5, and one card deeper for the Lantern a tutor takes out
        games=100_000,
    ),
    Question(
        "lantern",
        "lantern-route-tutors.criteria.toml",
        "Dizzy Spell transmuted by turn 5",
        # { turn = 5, cast = 'name:"Dizzy Spell"', min = 1 }
        _tutors_route(lambda p: p.cast_by(DIZZY, 5)),
        6,
        games=100_000,
    ),
    Question(
        "lantern",
        "lantern-route-tutors.criteria.toml",
        "Whir of Invention cast by turn 5",
        # { turn = 5, cast = 'name:"Whir of Invention"', min = 1 }
        _tutors_route(lambda p: p.cast_by(WHIR, 5)),
        6,
        games=100_000,
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


# --- Rocks and dorks in the line (ADR 0018) ----------------------------------
#
# The commander and the rocks or dorks, in one line. The lands-only pair beside
# each is the gate, which is what the same line reads with no source in it; the
# engine asks that half in the commander files, whose line names only the
# commander, and the rock half in files of their own, because naming a rock in
# a line moves every number that line answers.

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
# The engine samples every one of these, so its own error bar is most of the
# interval; 100,000 games keeps the checker's half of it under 0.52pp and the
# whole compare near five minutes rather than eight.
LINE_GAMES = 100_000

ROCK_QUESTIONS: list[Question] = (
    [
        Question(
            "lantern",
            "lantern-commander.criteria.toml",
            f"{RASHMI} castable by turn {t}, lands only",
            _cast_by(((RASHMI,),), RASHMI, t, 5),
            5,
            games=LINE_GAMES,
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
            games=LINE_GAMES,
        )
        for t in (4, 5)
    ]
    + [
        Question(
            "lantern",
            "lantern-rocks-first.criteria.toml",
            f"{RASHMI} cast by turn 5, rocks first in the line",
            _cast_by(LANTERN_ROCKS_FIRST_LINE, RASHMI, 5, 5),
            5,
            games=LINE_GAMES,
        ),
    ]
    + [
        Question(
            "loam",
            "loam-commander.criteria.toml",
            f"{BORBORYGMOS} castable by turn {t}, lands only",
            _cast_by(((BORBORYGMOS,),), BORBORYGMOS, t, 5),
            5,
            games=LINE_GAMES,
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
            games=LINE_GAMES,
        )
        for t in (4, 5)
    ]
)
QUESTIONS += ROCK_QUESTIONS


# --- The Loam north star (#101) -----------------------------------------------
#
# decks/loam.criteria.toml's one line, asked at turns 4 to 7: both halves at
# once, and each half on its own, of the same games.
NORTH_STAR = "Life from the Loam put into the graveyard and Borborygmos and Fblthp cast"
NORTH_STAR_QUESTIONS: list[Question] = [
    q
    for t in (4, 5, 6, 7)
    for q in (
        Question(
            "loam",
            "loam.criteria.toml",
            f"north star by turn {t}: {NORTH_STAR}",
            _north_star(t),
            NORTH_STAR_DEPTH,
            games=LINE_GAMES,
        ),
        Question(
            "loam",
            "loam.criteria.toml",
            f"Life from the Loam in the graveyard by turn {t}, north-star line",
            _north_star_loam(t),
            NORTH_STAR_DEPTH,
            games=LINE_GAMES,
        ),
        Question(
            "loam",
            "loam.criteria.toml",
            f"Borborygmos and Fblthp cast by turn {t}, north-star line",
            _north_star_commander(t),
            NORTH_STAR_DEPTH,
            games=LINE_GAMES,
        ),
    )
]
QUESTIONS += NORTH_STAR_QUESTIONS


# --- Running ------------------------------------------------------------------


def play(
    library: list[Card],
    questions: list[Question],
    on_the_draw: bool,
    games: int,
    seed: str,
    commanders: tuple[Card, ...] = (),
    state: tuple | None = None,
) -> dict[str, int]:
    """Deal `games` games from one seeded shuffle stream and count, per
    question, the games where it held. One deal answers every question, so
    the questions are correlated with each other but each is a fair estimate.

    `state` starts the stream part-way along instead of at `seed`: see
    `stream_chunks`, which is how compare.py splits one stream across
    processes without changing a single deal."""
    rng = random.Random(seed)  # str seeds are hashed deterministically
    if state is not None:
        rng.setstate(state)
    depth = deal_depth(questions)
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


def deal_depth(questions: list[Question]) -> int:
    """How many cards off the top one deal takes: the seven, and enough draws
    for the deepest question on either seat."""
    return 7 + max(q.deepest_turn for q in questions)


def stream_chunks(
    seed: str, library_size: int, depth: int, games: int, chunk: int
) -> Iterator[tuple[tuple, int]]:
    """Cut the stream `play(..., games, seed)` deals into runs of `chunk` games,
    as (the generator's state at the run's first game, games in the run).

    Playing every run from its state deals exactly the games one `play` call
    would, in the same order, so the counts they add up to are the same to the
    game: the split is a matter of time, not of numbers. Finding each state
    means drawing the shuffles themselves, which is cheap beside asking the
    questions of them. A sample's draws depend only on the population's size
    and the depth, never on the cards, so sampling positions stands in for
    sampling the library."""
    rng = random.Random(seed)
    positions = range(library_size)
    for start in range(0, games, chunk):
        n = min(chunk, games - start)
        yield rng.getstate(), n
        for _ in range(n):
            rng.sample(positions, depth)


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
