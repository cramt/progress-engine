"""What each Lantern tutor route is worth to the owner's north star (#80).

The north star: by the end of turn 5, Lantern of Insight is on the battlefield
by any route AND Rashmi and Ragavan has been cast from the command zone, out of
one mana budget. Opponents neither help nor hinder.

This is a throwaway measuring stick for ADR 0019, not a checker and not an
engine; it lives outside crates/ and reads nothing in them. It deals shuffles
of `decks/lantern.txt` and asks, per deal, whether SOME line of play reaches
the north star using only the routes switched on. The pilot is clairvoyant
about their own library (a depth-first search over every line), so each level
is an upper bound on what a declared line reads; docs/research/suspect-numbers.md
item 4 prices that kind of hindsight at under 1.3 points. What the ADR uses is
the DIFFERENCE between route sets, and every set is played on the same deals.

Mana, as the engine will count it once #82 and ADR 0018 ship:
  * a fetchland pays the colours of the untapped lands it can find; Maze of Ith
    pays nothing; Castle Doom and Spire of Industry pay {C} (suspect-numbers);
  * conditional taplands enter tapped; Urza's Saga taps for {C} until the turn
    its chapter III resolves, and on that turn;
  * with --rocks, Sol Ring (2), Mind Stone, Arcane Signet and the three
    Talismans are sources once cast, and a rock's mana pays only for what is
    cast after it that turn (ADR 0018). Fellwar Stone makes nothing. Without
    --rocks, lands are the only sources, which is the engine today.
Improvise, Treasure and Rashmi's trigger are not counted.

    python3 docs/research/tutor-routes.py decks/index.jsonl decks/lantern.txt \\
        [--draw] [--rocks] [--no-commander] [--games N] [--sequence a,b+c,...]

Standard library only.
"""

from __future__ import annotations

import json
import random
import re
import sys
from functools import lru_cache

LANTERN = "Lantern of Insight"
FETCH_PALETTE = {  # suspect-numbers.md item 1: what an untapped find can make
    "Scalding Tarn": "RU",
    "Misty Rainforest": "GU",
    "Wooded Foothills": "GRU",
    "Prismatic Vista": "GRU",
}
PALETTE_OVERRIDE = {"Maze of Ith": "", "Castle Doom": "C", "Spire of Industry": "C"}
ROUTE_CARDS = {
    LANTERN, "Trinket Mage", "Fabricate", "Tezzeret, Cruel Captain", "Dizzy Spell",
    "Artificer's Intuition", "Tezzeret the Seeker", "Whir of Invention",
    "Expedition Map", "Repurposing Bay", "Goblin Engineer",
}
# ADR 0018's standard library, on this list: (cost, the palette of each mana)
ROCKS = {
    "Sol Ring": ((1, ""), ("C", "C")),
    "Mind Stone": ((2, ""), ("C",)),
    "Arcane Signet": ((2, ""), ("GRU",)),
    "Talisman of Creativity": ((2, ""), ("CRU",)),
    "Talisman of Curiosity": ((2, ""), ("CGU",)),
    "Talisman of Impulse": ((2, ""), ("CGR",)),
}
COST = {  # (generic, pips)
    LANTERN: (1, ""),
    "commander": (1, "GRU"),
    "Trinket Mage": (2, "U"),
    "Fabricate": (2, "U"),
    "Tezzeret, Cruel Captain": (3, ""),
    "Dizzy Spell": (1, "UU"),  # the transmute, not the printed {U}
    "Intuition cast": (1, "U"),
    "Intuition act": (0, "U"),
    "Tezzeret the Seeker": (3, "UU"),
    "Whir of Invention": (1, "UUU"),  # X = 1, improvise not counted
    "Map cast": (1, ""),
    "Map act": (2, ""),
    "Bay cast": (2, "U"),
    "Bay act": (2, ""),
    "Engineer cast": (1, "R"),
    "Engineer act": (0, "R"),
    "Fair act": (4, ""),
}
TO_HAND = {"Trinket Mage": "trinket", "Fabricate": "fabricate",
           "Tezzeret, Cruel Captain": "cruel", "Dizzy Spell": "dizzy"}
TO_FIELD = {"Tezzeret the Seeker": "seeker", "Whir of Invention": "whir"}


def load(index_path, deck_path, rocks):
    idx = {}
    with open(index_path, encoding="utf-8") as f:
        next(f)
        for line in f:
            k, _, rec = line.rstrip("\n").partition("\t")
            idx[k] = rec
    library = []
    for raw in open(deck_path, encoding="utf-8"):
        line = raw.strip()
        if not line or line.startswith("//") or "Commander" in line:
            continue
        name = re.match(r"^(\d+)x?\s+(.+?)\s+\(", line).group(2)
        c = json.loads(idx[name.lower()])
        faces = c.get("faces") or [{"type_line": c["type_line"]}]
        front = faces[0]["type_line"]
        mdfc_land = c.get("layout") == "modal_dfc" and any("Land" in f["type_line"] for f in faces)
        tags = set(c.get("tags") or [])
        if "Land" in front or mdfc_land:
            if name in FETCH_PALETTE:
                pal, tapped = FETCH_PALETTE[name], False
            else:
                pal = PALETTE_OVERRIDE.get(name, "".join(sorted(c.get("produces") or [])))
                tapped = bool(tags & {"tapland", "conditional-tapland"})
            special = {"Urza's Saga": "saga", "Inventors' Fair": "fair"}.get(name, "")
            library.append(("L", pal, tapped, "Artifact" in front, special))
        elif name in ROUTE_CARDS or (rocks and name in ROCKS):
            library.append(("S", name))
        elif "Artifact" in front:
            library.append(("A",))  # an artifact card: discard fodder, nothing else
        else:
            library.append(("X",))  # read by no route
    assert len(library) == 99, len(library)
    return library


def payable(sources, bills):
    """Can `sources` pay every bill? A source is (palette, avail) and a bill is
    (generic, pips, k): a source pays a bill only if avail < k, which is how a
    rock cast mid-turn pays only for what came after it (ADR 0018). Lands and
    older rocks have avail -1."""
    need = sum(g + len(p) for g, p, _ in bills)
    if need > len(sources):
        return False
    pips = [(p, k) for _, ps, k in bills for p in ps]
    order = sorted(range(len(bills)), key=lambda i: bills[i][2])

    def generic_fits(used):
        free = [a for j, (_, a) in enumerate(sources) if not used >> j & 1]
        taken = 0
        for i in order:
            g, _, k = bills[i]
            taken += g
            if sum(1 for a in free if a < k) < taken:
                return False
        return True

    def go(i, used):
        if i == len(pips):
            return generic_fits(used)
        p, k = pips[i]
        for j, (pal, a) in enumerate(sources):
            if not used >> j & 1 and a < k and p in pal:
                if go(i + 1, used | 1 << j):
                    return True
        return False

    return go(0, 0)


def reaches(deck, on_draw, routes, turns=5, commander=True):
    """Whether some line reaches the north star on this deal."""
    rest = deck[7:]
    start = tuple(sorted(c for c in deck[:7] if c != ("X",)))

    def lantern_at(pos, gone):
        for j in range(pos, len(rest)):
            if j not in gone and rest[j] == ("S", LANTERN):
                return j
        return None

    def saga_at(pos, gone):
        for j in range(pos, len(rest)):
            if j not in gone and rest[j][0] == "L" and rest[j][4] == "saga":
                return j
        return None

    @lru_cache(maxsize=None)
    def turn(t, hand, field, pos, gone, st):
        # st = (lantern_bf, cmdr, saga_due, map_bf, bay_bf, eng_ready, lantern_yard, intu_bf)
        if st[0] and st[1]:
            return True
        if t > turns:
            return False
        hand = list(hand)
        if t > 1 or on_draw:
            while pos < len(rest) and pos in gone:
                pos += 1
            if pos < len(rest):
                if rest[pos] != ("X",):
                    hand.append(rest[pos])
                pos += 1
        saga_today = False
        if st[2] == t:  # chapter III, after the draw and before the land drop
            j = lantern_at(pos, gone)
            lantern_bf = st[0]
            if j is not None and not lantern_bf:
                gone, lantern_bf = gone | {j}, True
            st = (lantern_bf, st[1], 0) + st[3:]
            saga_today = True
        return step(t, tuple(sorted(hand)), field, pos, gone, st, False, (), 0, saga_today)

    @lru_cache(maxsize=None)
    def step(t, hand, field, pos, gone, st, dropped, bills, k, saga_today):
        """`field` holds lands as ("L", ...) with the turn they entered, and
        rocks as ("R", name, palettes) with the rock-index they were cast at
        this turn (-1 for an earlier turn). `bills` is what this turn has paid
        so far and `k` how many rocks it has cast."""
        lantern_bf, cmdr, saga_due, map_bf, bay_bf, eng_ready, lantern_yard, intu_bf = st
        if lantern_bf and cmdr:
            return True

        def sources(skip=()):
            out = []
            for i, (card, e) in enumerate(field):
                if i in skip:
                    continue
                if card[0] == "R":
                    out.extend((pal, e) for pal in card[2])
                    continue
                _, pal, tapped, _, special = card
                if tapped and e == t:
                    continue
                if special.startswith("gone") and special != f"gone{t}":
                    continue
                if pal:
                    out.append((pal, -1))
            return out

        def can(cost, skip=()):
            nb = bills + ((cost[0], cost[1], k),)
            return payable(sources(skip), nb), nb

        def go(hand2=hand, field2=field, gone2=gone, st2=st, dropped2=dropped, bills2=bills, k2=k):
            return step(t, tuple(sorted(hand2)), tuple(field2), pos, gone2, st2, dropped2,
                        bills2, k2, saga_today)

        def with_st(**kw):
            names = ("lantern_bf", "cmdr", "saga_due", "map_bf", "bay_bf", "eng_ready",
                     "lantern_yard", "intu_bf")
            d = dict(zip(names, st))
            d.update(kw)
            return tuple(d[n] for n in names)

        hl = list(hand)

        def without(*cards):
            h = list(hl)
            for c in cards:
                h.remove(c)
            return h

        # End the turn. The board is put in a canonical order so that two
        # lines reaching the same board share one memo entry.
        end = []
        for card, e in field:
            if card[0] == "R":
                end.append((card, -1))
            elif not (card[4] == "saga" and saga_today) and not card[4].startswith("gone"):
                end.append((card, 0))
        if turn(t + 1, hand, tuple(sorted(end)), pos, gone, st):
            return True

        # The land drop.
        if not dropped:
            for card in set(c for c in hl if c[0] == "L"):
                st2 = st
                if card[4] == "saga" and "saga" in routes and not saga_due:
                    st2 = with_st(saga_due=t + 2)
                if go(hand2=without(card), field2=field + ((card, t),), st2=st2, dropped2=True):
                    return True

        # The commander, from the command zone (#78).
        if not cmdr:
            ok, nb = can(COST["commander"])
            if ok and go(bills2=nb, st2=with_st(cmdr=True)):
                return True

        # Rocks (ADR 0018): cast, and a source for what comes after them.
        if "rocks" in routes:
            for name in {c[1] for c in hl if c[0] == "S" and c[1] in ROCKS}:
                cost, pals = ROCKS[name]
                ok, nb = can(cost)
                if ok and go(hand2=without(("S", name)), field2=field + ((("R", name, pals), k),),
                             bills2=nb, k2=k + 1):
                    return True

        # Lantern from hand.
        if ("S", LANTERN) in hl and not lantern_bf:
            ok, nb = can(COST[LANTERN])
            if ok and go(hand2=without(("S", LANTERN)), bills2=nb, st2=with_st(lantern_bf=True)):
                return True
        if lantern_bf:
            return False  # the commander above is all that is left to do

        j = lantern_at(pos, gone)
        # Tutors that put it in hand, and tutors that put it onto the field.
        for name, key in TO_HAND.items():
            if key in routes and ("S", name) in hl and j is not None:
                ok, nb = can(COST[name])
                if ok and go(hand2=without(("S", name)) + [("S", LANTERN)], gone2=gone | {j}, bills2=nb):
                    return True
        for name, key in TO_FIELD.items():
            if key in routes and ("S", name) in hl and j is not None:
                ok, nb = can(COST[name])
                if ok and go(hand2=without(("S", name)), gone2=gone | {j}, bills2=nb,
                             st2=with_st(lantern_bf=True)):
                    return True

        # Artificer's Intuition: cast {1}{U}; then {U} and discard an artifact.
        if "intuition" in routes:
            if not intu_bf and ("S", "Artificer's Intuition") in hl:
                ok, nb = can(COST["Intuition cast"])
                if ok and go(hand2=without(("S", "Artificer's Intuition")), bills2=nb,
                             st2=with_st(intu_bf=True)):
                    return True
            if intu_bf and j is not None:
                fodder = sorted({c for c in hl if c == ("A",) or (c[0] == "L" and c[3])
                                 or (c[0] == "S" and (c[1] in ROCKS or c[1] in
                                                      ("Expedition Map", "Repurposing Bay")))})
                for f in fodder:
                    ok, nb = can(COST["Intuition act"])
                    if ok and go(hand2=without(f) + [("S", LANTERN)], gone2=gone | {j}, bills2=nb):
                        return True

        # Expedition Map: cast {1}; then {2},{T}, sacrifice it: Urza's Saga to hand.
        if "map" in routes and "saga" in routes:
            if not map_bf and ("S", "Expedition Map") in hl:
                ok, nb = can(COST["Map cast"])
                if ok and go(hand2=without(("S", "Expedition Map")), bills2=nb, st2=with_st(map_bf=True)):
                    return True
            # "map-late": the walk plays its line after the land drop, so an
            # activation cannot come before the drop it fetched a land for.
            if map_bf and (dropped or "map-late" not in routes):
                s = saga_at(pos, gone)
                if s is not None:
                    ok, nb = can(COST["Map act"])
                    if ok and go(hand2=hl + [rest[s]], gone2=gone | {s}, bills2=nb,
                                 st2=with_st(map_bf=False)):
                        return True

        # Repurposing Bay: cast {2}{U}; then {2},{T}, sacrifice a mana value 0
        # artifact (an artifact land, tapped for mana first): Lantern onto the field.
        if "bay" in routes:
            if not bay_bf and ("S", "Repurposing Bay") in hl:
                ok, nb = can(COST["Bay cast"])
                if ok and go(hand2=without(("S", "Repurposing Bay")), bills2=nb, st2=with_st(bay_bf=True)):
                    return True
            if bay_bf and j is not None:
                for i, (c, e) in enumerate(field):
                    if c[0] == "L" and c[3] and not c[4].startswith("gone"):
                        ok, nb = can(COST["Bay act"])
                        if ok:
                            f2 = list(field)
                            f2[i] = (("L", c[1], c[2], False, f"gone{t}"), e)
                            if go(field2=f2, gone2=gone | {j}, bills2=nb, st2=with_st(lantern_bf=True)):
                                return True
                        break

        # Goblin Engineer: cast {1}{R}, the Lantern to the graveyard; from the
        # next turn, {R},{T}, sacrifice an artifact: back onto the field.
        if "engineer" in routes:
            if not eng_ready and ("S", "Goblin Engineer") in hl and j is not None:
                ok, nb = can(COST["Engineer cast"])
                if ok and go(hand2=without(("S", "Goblin Engineer")), gone2=gone | {j}, bills2=nb,
                             st2=with_st(eng_ready=t + 1, lantern_yard=True)):
                    return True
            if eng_ready and eng_ready <= t and lantern_yard:
                for i, (c, e) in enumerate(field):
                    if (c[0] == "L" and c[3] and not c[4].startswith("gone")) or c[0] == "R":
                        ok, nb = can(COST["Engineer act"])
                        if ok:
                            f2 = list(field)
                            if c[0] == "L":
                                f2[i] = (("L", c[1], c[2], False, f"gone{t}"), e)
                            else:
                                del f2[i]  # a rock: its mana is gone with it
                            if go(field2=f2, bills2=nb,
                                  st2=with_st(lantern_bf=True, lantern_yard=False, eng_ready=0)):
                                return True
                        break

        # Inventors' Fair: {4},{T}, sacrifice it, three artifacts: to hand.
        if "fair" in routes and j is not None:
            fairs = [i for i, (c, e) in enumerate(field) if c[0] == "L" and c[4] == "fair"]
            arts = (sum(1 for c, e in field if c[0] == "R" or (c[0] == "L" and c[3]))
                    + int(map_bf) + int(bay_bf))
            if fairs and arts >= 3:
                ok, nb = can(COST["Fair act"], skip=(fairs[0],))
                if ok:
                    f2 = list(field)
                    c, e = f2[fairs[0]]
                    f2[fairs[0]] = (("L", "", False, False, f"gone{t}"), e)
                    if go(hand2=hl + [("S", LANTERN)], field2=f2, gone2=gone | {j}, bills2=nb):
                        return True
        return False

    return turn(1, start, (), 0, frozenset(),
                (False, not commander, 0, False, False, 0, False, False))


TODAY = {"trinket", "fabricate", "cruel", "saga"}
CANDIDATES = ["seeker", "whir", "dizzy", "intuition", "map", "bay", "engineer", "fair"]


def main():
    args = sys.argv[1:]
    flag = lambda f: f in args
    value = lambda f, d: args[args.index(f) + 1] if f in args else d
    on_draw, rocks, commander = flag("--draw"), flag("--rocks"), not flag("--no-commander")
    games = int(value("--games", 4000))
    files = [a for i, a in enumerate(args)
             if not a.startswith("--") and (i == 0 or args[i - 1] not in ("--games", "--sequence"))]
    library = load(files[0], files[1], rocks)
    today = TODAY | ({"rocks"} if rocks else set())
    if flag("--sequence"):
        sets, acc = {"today (+#78)": set(today)}, set(today)
        for r in value("--sequence", "").split(","):
            acc = acc | set(r.split("+"))
            sets[f"+ {r}"] = set(acc)
    else:
        everything = today | set(CANDIDATES)
        sets = {"hard-cast only": {"rocks"} & today, "today (+#78)": today, "everything": everything}
        for c in CANDIDATES:
            sets[f"today + {c}"] = today | {c}
        for c in sorted(everything - {"rocks"}):
            sets[f"everything - {c}"] = everything - {c}
    rng = random.Random(80)
    deals = []
    for _ in range(games):
        d = list(library)
        rng.shuffle(d)
        deals.append(d)
    from multiprocessing import Pool

    jobs = [(d, on_draw, frozenset(r), 5, commander) for r in sets.values() for d in deals]
    with Pool() as pool:
        flat = pool.starmap(reaches, jobs, chunksize=32)
    results = {label: flat[i * games:(i + 1) * games] for i, label in enumerate(sets)}
    labels = list(results)
    print(f"{'draw' if on_draw else 'play'}, {games} deals (seed 80), same deals for every row; "
          f"{'with' if commander else 'WITHOUT'} the commander; "
          f"{'rocks counted' if rocks else 'lands only'}")
    for i, label in enumerate(labels):
        hits = results[label]
        if flag("--sequence"):
            ref = results[labels[max(i - 1, 0)]]
        elif label.startswith("everything -"):
            ref = results["everything"]
        else:
            ref = results["today (+#78)"]
        gain = sum(1 for a, b in zip(hits, ref) if a and not b) / games
        loss = sum(1 for a, b in zip(hits, ref) if b and not a) / games
        print(f"  {label:26} {100 * sum(hits) / games:6.2f}%   +{100 * gain:.2f} / -{100 * loss:.2f} pp")


if __name__ == "__main__":
    main()
