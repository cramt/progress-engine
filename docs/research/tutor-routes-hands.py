"""The numbers HANDS.md hands 40-42 pin, computed by brute force (#80).

Every distinct order of a small deck is dealt and played out under the
semantics ADR 0019 decides, with declared priorities and no clairvoyance: the
land drop plays the Saga first and then any Island, and the line walks its list
in order after the land drop, paying an activation of a card it already put
into play or casting one from hand, whichever the pool still covers, and reads
the list again from the top after each (ADR 0017, ADR 0018). The one thing
paid before the drop is ADR 0019's exception: an activation fetching a land the
drop list ranks above every land in hand (the Map, for a Saga). The answer is
the share of orders that reach the row. Nothing here reads the engine.

    python3 docs/research/tutor-routes-hands.py
"""

from __future__ import annotations

from fractions import Fraction
from math import factorial
from collections import Counter

ISLAND, BOLT = "Island", "Lightning Bolt"
LANTERN, SAGA, MAP = "Lantern of Insight", "Urza's Saga", "Expedition Map"
SEEKER, DIZZY = "Tezzeret the Seeker", "Dizzy Spell"


def orders(deck):
    """Distinct orders of a multiset deck, each with its probability."""
    counts = Counter(deck)
    total = factorial(len(deck))
    for c in counts.values():
        total //= factorial(c)
    names = sorted(counts)
    left = [counts[n] for n in names]
    out = []

    def go():
        if len(out) == len(deck):
            yield tuple(out)
            return
        for i, n in enumerate(names):
            if left[i]:
                left[i] -= 1
                out.append(n)
                yield from go()
                out.pop()
                left[i] += 1

    for p in go():
        yield p, Fraction(1, total)


def play(order, on_draw, turns, line, effects):
    """One game. `line` is the [casting] list; `effects` maps a card to what
    it does: ("fetch", cost or None, target, dest) for a card whose play
    fetches, or ("activate", cast_cost, act_cost, target, dest) for a card
    whose activation, paid later, does. A cost is (generic, blue pips).
    Returns per-turn snapshots."""
    library = list(order[7:])
    hand = Counter(order[:7])
    lands = 0  # Islands in play, all untapped from the turn after... Islands
    saga_due = None  # enter untapped, so every land pays the turn it lands
    saga_in_play = False
    saga_played = False
    field = Counter()
    armed = Counter()  # permanents in play with an unpaid activation
    cast = Counter()
    snaps = []
    for t in range(1, turns + 1):
        if t > 1 or on_draw:
            if library:
                hand[library.pop(0)] += 1
        pool_bonus = 0
        if saga_due == t:
            if LANTERN in library:
                library.remove(LANTERN)
                field[LANTERN] += 1
            saga_in_play = False
            pool_bonus = 1  # tapped for {C} with chapter III on the stack
            saga_due = None
        # ADR 0019: before the drop, an activation that fetches a land the
        # [land_drop] list ranks above every land in hand is paid out of the
        # sources already in play. Here that is the Map, for a Saga, when the
        # hand holds none.
        pre_spent = 0
        for card in line:
            eff = effects.get(card)
            if (eff and eff[0] == "activate" and armed[card] and eff[3] == SAGA
                    and not hand[SAGA] and SAGA in library and saga_due is None
                    and not saga_in_play):
                have = lands + pool_bonus
                if eff[2][0] + eff[2][1] <= have:
                    armed[card] -= 1
                    pre_spent = eff[2][0] + eff[2][1]
                    library.remove(SAGA)
                    hand[SAGA] += 1
                break
        # the land drop: the Saga first, then an Island
        if hand[SAGA] and saga_due is None and not saga_in_play:
            hand[SAGA] -= 1
            saga_in_play, saga_due = True, t + 2
            saga_played = True
        elif hand[ISLAND]:
            hand[ISLAND] -= 1
            lands += 1
        mana = lands + (1 if saga_in_play else 0) + pool_bonus
        blue = lands
        spent, spent_blue = pre_spent, pre_spent  # Islands paid it: blue spent

        def pay(cost):
            nonlocal spent, spent_blue
            g, u = cost
            if spent + g + u <= mana and spent_blue + u <= blue:
                spent += g + u
                spent_blue += u
                return True
            return False

        def resolve(eff):
            target, dest = eff[-2], eff[-1]
            if target in library:
                library.remove(target)
                (hand if dest == "hand" else field)[target] += 1

        def act_once():
            """The first thing the line can still do: for each entry in
            order, an activation of a copy already in play (that copy was
            already bought), then a copy cast from hand."""
            for card in line:
                eff = effects.get(card)
                if eff and eff[0] == "activate" and armed[card] and pay(eff[2]):
                    armed[card] -= 1
                    resolve(eff)
                    return True
                if hand[card]:
                    if eff and eff[0] == "fetch" and eff[1] is not None:
                        cost = eff[1]
                    elif eff and eff[0] == "activate":
                        cost = eff[1]
                    else:
                        cost = {LANTERN: (1, 0), SEEKER: (3, 2), DIZZY: (0, 1),
                                MAP: (1, 0)}[card]
                    if pay(cost):
                        hand[card] -= 1
                        cast[card] += 1
                        if card == LANTERN:
                            field[LANTERN] += 1
                        if eff and eff[0] == "fetch":
                            resolve(eff)
                        elif eff and eff[0] == "activate":
                            armed[card] += 1  # not a creature: usable at once
                        return True
            return False

        # The line is read again from its top after everything it does
        # (ADR 0017, ADR 0018), so a card a tutor just put in hand is cast
        # this turn if the pool still pays for it.
        while act_once():
            pass
        snaps.append(dict(field=Counter(field), cast=Counter(cast), hand=Counter(hand),
                          library=Counter(library), saga_played=saga_played))
    return snaps


def table(title, deck, on_draw, turns, rows, variants):
    print(f"\n{title} ({'draw' if on_draw else 'play'}, {len(deck)} cards)")
    head = "".join(f"{v:>22}" for v in variants)
    print(f"  {'':48}{head}")
    results = {v: {r: Fraction(0) for r in rows} for v in variants}
    for order, p in orders(deck):
        for v, (line, effects) in variants.items():
            snaps = play(order, on_draw, turns, line, effects)
            for r, test in rows.items():
                if test(snaps):
                    results[v][r] += p
    for r in rows:
        cells = "".join(f"{str(results[v][r]):>12} {float(results[v][r]) * 100:7.2f}%"
                        for v in variants)
        print(f"  {r:48}{cells}")


def main():
    # Hand 40: Tezzeret the Seeker puts it onto the battlefield.
    deck = [ISLAND] * 10 + [SEEKER, LANTERN]
    line = [LANTERN, SEEKER]
    table("hand 40, Tezzeret the Seeker", deck, False, 5, {
        "Tezzeret the Seeker cast by turn 5": lambda s: s[4]["cast"][SEEKER] >= 1,
        "Lantern on the battlefield by turn 5": lambda s: s[4]["field"][LANTERN] >= 1,
        "Lantern cast by turn 5": lambda s: s[4]["cast"][LANTERN] >= 1,
        "Lantern still in the library on turn 5": lambda s: s[4]["library"][LANTERN] >= 1,
    }, {
        "no fetch": (line, {}),
        "fetch to battlefield": (line, {SEEKER: ("fetch", None, LANTERN, "battlefield")}),
    })

    # Hand 41: Dizzy Spell's transmute, at its printed cost and at the
    # declared one.
    deck = [ISLAND] * 10 + [DIZZY, LANTERN]
    line = [LANTERN, DIZZY]
    table("hand 41, Dizzy Spell", deck, False, 4, {
        "Dizzy Spell played by turn 2": lambda s: s[1]["cast"][DIZZY] >= 1,
        "Lantern cast by turn 2": lambda s: s[1]["cast"][LANTERN] >= 1,
        "Lantern cast by turn 4": lambda s: s[3]["cast"][LANTERN] >= 1,
    }, {
        "printed {U}": (line, {DIZZY: ("fetch", (0, 1), LANTERN, "hand")}),
        "declared {1}{U}{U}": (line, {DIZZY: ("fetch", (1, 2), LANTERN, "hand")}),
    })

    # Hand 42: Expedition Map goes and gets Urza's Saga. Hand 17's sixteen
    # cards with two Bolts traded for the Map and a fifth Island.
    # The line names the Map and nothing else, so the Lantern only ever
    # arrives by chapter III, and the Map entry also pays the Map's
    # activation once the Map is in play.
    deck = [ISLAND] * 5 + [BOLT] * 8 + [MAP, SAGA, LANTERN]
    line = [MAP]
    table("hand 42, Expedition Map", deck, False, 5, {
        "Expedition Map cast on turn 1": lambda s: s[0]["cast"][MAP] >= 1,
        "Urza's Saga played by turn 3": lambda s: s[2]["saga_played"],
        "Lantern on the battlefield by turn 5": lambda s: s[4]["field"][LANTERN] >= 1,
    }, {
        "Map never activated": (line, {}),
        "activation {2}, Saga to hand": (line, {MAP: ("activate", (1, 0), (2, 0), SAGA, "hand")}),
    })


if __name__ == "__main__":
    main()
