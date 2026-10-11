/**
 * How the sourcing and build views group what `chip-decklist`'s `supply`
 * worked out. Which copy answers which line is Rust's; this only sorts the
 * answer into the sections the views show.
 */
import type { CardRef, Finish } from "../deck";
import type {
  ParsedSupply,
  SupplyCopies,
  SupplyLine,
  SupplyShort,
  Whereabouts,
} from "../supply";
import type { TradeOffer } from "../wanted";

export type ReadSupply = Extract<ParsedSupply, { kind: "supply" }>;

/** Some of a line's copies, from one collection line. */
export interface Row {
  line: SupplyLine;
  copies: SupplyCopies;
}

/** `CMM 400 · foil`, or what of it the copy says; `""` for a plain card by name. */
export function printingLabel(card: CardRef, finish: Finish): string {
  const parts: string[] = [];
  if (card.kind === "printing")
    parts.push(`${card.set.toUpperCase()} ${card.num}`);
  if (finish !== "nonfoil") parts.push(finish);
  return parts.join(" · ");
}

/** Where a copy is, as the person holding the deck would say it. */
export function whereLabel(c: {
  at?: string;
  whereabouts: Whereabouts;
}): string {
  switch (c.whereabouts) {
    case "thisDeck":
      return "In the deck";
    case "otherDeck":
      return `In ${c.at ?? "another deck"}`;
    case "loose":
      return c.at ?? "Unsorted";
  }
}

const byName = (a: { name: string }, b: { name: string }) =>
  a.name.localeCompare(b.name, undefined, { sensitivity: "base" });

function rows(s: ReadSupply, keep: (c: SupplyCopies) => boolean): Row[] {
  return s.lines
    .flatMap((line) =>
      line.copies.filter(keep).map((copies) => ({ line, copies })),
    )
    .sort((a, b) => byName(a.line, b.line));
}

const copiesIn = (list: readonly Row[]) =>
  list.reduce((n, r) => n + r.copies.qty, 0);
const shortIn = (list: readonly SupplyShort[]) =>
  list.reduce((n, s) => n + s.short, 0);

/** The deck by where each copy is to come from. */
export function sourcing(s: ReadSupply) {
  const have = rows(s, (c) => c.exact);
  const alike = rows(s, (c) => !c.exact);
  const short = [...s.short].sort(byName);
  const friends = short.filter((c) => c.friends.length > 0);
  const buy = short.filter((c) => c.friends.length === 0 && !c.basic);
  const basics = short.filter((c) => c.friends.length === 0 && c.basic);
  return {
    have,
    alike,
    friends,
    buy,
    basics,
    copies: {
      need: s.lines.reduce((n, l) => n + l.need, 0),
      have: copiesIn(have),
      alike: copiesIn(alike),
      friends: shortIn(friends),
      buy: shortIn(buy),
      basics: shortIn(basics),
    },
  };
}

/** What of the short cards `who` holds, as a trade lists it. */
export function offersOf(s: ReadSupply, who: string): TradeOffer[] {
  return [...s.short].sort(byName).flatMap((c) => {
    const has = c.friends.find((f) => f.who === who);
    return has ? [{ name: c.name, short: c.short, pulls: has.pulls }] : [];
  });
}

/** One place to pull copies from, and what to take out of it. */
export interface Pile {
  at: string | undefined;
  whereabouts: Whereabouts;
  rows: Row[];
}

/**
 * The copies to bring to the deck, place by place: binders and boxes in name
 * order, then the unsorted ones, then those in another deck, which are
 * taken apart last.
 */
export function piles(s: ReadSupply): Pile[] {
  const out: Pile[] = [];
  for (const row of rows(s, (c) => c.whereabouts !== "thisDeck")) {
    const { at, whereabouts } = row.copies;
    let pile = out.find((p) => p.at === at && p.whereabouts === whereabouts);
    if (!pile) {
      pile = { at, whereabouts, rows: [] };
      out.push(pile);
    }
    pile.rows.push(row);
  }
  const rank = (p: Pile) =>
    p.whereabouts === "otherDeck" ? 2 : p.at === undefined ? 1 : 0;
  return out.sort(
    (a, b) =>
      rank(a) - rank(b) ||
      (a.at ?? "").localeCompare(b.at ?? "", undefined, {
        sensitivity: "base",
      }),
  );
}

/** The copies already in the deck, by name: what to look for in it. */
export function sleeved(s: ReadSupply): Row[] {
  return rows(s, (c) => c.whereabouts === "thisDeck");
}

/** How far the physical deck is from the list. */
export function progress(s: ReadSupply) {
  const need = s.lines.reduce((n, l) => n + l.need, 0);
  const inDeck = copiesIn(sleeved(s));
  const exact = copiesIn(sleeved(s).filter((r) => r.copies.exact));
  return {
    need,
    inDeck,
    /** Of those in the deck, the ones not the printing the list names. */
    standIns: inDeck - exact,
    toPull: copiesIn(rows(s, (c) => c.whereabouts !== "thisDeck")),
    toTakeOut: s.extra.reduce((n, e) => n + e.qty, 0),
    missing: shortIn(s.short),
  };
}
