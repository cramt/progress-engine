/**
 * The deck by what it costs, for deciding per card whether to buy it, proxy
 * it, swap it for a cheaper printing or just ask the playgroup for one.
 */
import type { PrintingOption } from "../card/prints";
import type { Card, Finish } from "../deck";
import { type PriceBook, unitPrice } from "../prices";
import type { Currency } from "../scryfall";

/** Dearest first, which is the order the page shows them in. */
export const TIERS = ["proxy", "buy", "ask"] as const;
export type Tier = (typeof TIERS)[number];

/**
 * Where the tiers split, in the deck's currency: a copy under `ask` is one to
 * ask around for, and one at `proxy` or over is one to think about proxying.
 * Only `bands` makes one, so `ask` is never above `proxy`.
 */
export interface Bands {
  readonly ask: number;
  readonly proxy: number;
}

export function bands(ask: number, proxy: number): Bands | null {
  return Number.isFinite(ask) &&
    Number.isFinite(proxy) &&
    ask >= 0 &&
    ask <= proxy
    ? { ask, proxy }
    : null;
}

export const DEFAULT_BANDS: Bands = { ask: 1, proxy: 10 };

const KEY = "meldweb.cost-bands";

/** The bands last set in this browser, or the defaults until some are. */
export function loadBands(
  storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage,
): Bands {
  try {
    const saved = JSON.parse(storage?.getItem(KEY) ?? "null") as unknown;
    const { ask, proxy } = (saved ?? {}) as Record<string, unknown>;
    return (
      (typeof ask === "number" &&
        typeof proxy === "number" &&
        bands(ask, proxy)) ||
      DEFAULT_BANDS
    );
  } catch {
    return DEFAULT_BANDS;
  }
}

export function saveBands(
  at: Bands,
  storage: Pick<Storage, "setItem"> | undefined = globalThis.localStorage,
): void {
  try {
    storage?.setItem(KEY, JSON.stringify(at));
  } catch {
    // blocked storage: the bands last as long as the page
  }
}

export function tierOf(each: number, at: Bands): Tier {
  if (each >= at.proxy) return "proxy";
  if (each < at.ask) return "ask";
  return "buy";
}

export interface Priced {
  card: Card;
  /** One copy, in the line's own finish. */
  each: number;
  /** Every copy on the line. */
  total: number;
}

export interface Breakdown {
  /** Every tier, each dearest first, and empty ones kept so a page can say so. */
  tiers: Record<Tier, { lines: Priced[]; total: number; qty: number }>;
  /** The deck's cards Scryfall has no price for in their finish. */
  unpriced: Card[];
  total: number;
  /** Copies outside the deck (maybeboard, sideboard…), which none of this counts. */
  outside: number;
}

/**
 * Each line in the deck once, whatever categories it is in: a card in two
 * categories is still bought once.
 */
export function breakdown(
  cards: readonly Card[],
  prices: PriceBook,
  currency: Currency,
  at: Bands,
  nameOf: (card: Card) => string,
): Breakdown {
  const tiers = Object.fromEntries(
    TIERS.map((t) => [t, { lines: [] as Priced[], total: 0, qty: 0 }]),
  ) as Breakdown["tiers"];
  const unpriced: Card[] = [];
  let total = 0;
  let outside = 0;
  for (const card of cards) {
    if (!card.inDeck) {
      outside += card.qty;
      continue;
    }
    const each = unitPrice(card, prices, currency);
    if (each === undefined) {
      unpriced.push(card);
      continue;
    }
    const tier = tiers[tierOf(each, at)];
    tier.lines.push({ card, each, total: each * card.qty });
    tier.total += each * card.qty;
    tier.qty += card.qty;
    total += each * card.qty;
  }
  for (const t of TIERS)
    tiers[t].lines.sort(
      (a, b) => b.each - a.each || nameOf(a.card).localeCompare(nameOf(b.card)),
    );
  return { tiers, unpriced, total, outside };
}

/**
 * The priced lines with each on its cheapest printing, where one is known and
 * cheaper than its own: what `b.total` would be. A line with no price of its
 * own is left out, as it is from `b.total`, so the two compare like for like.
 */
export function atCheapest(
  b: Breakdown,
  cheapestOf: (card: Card) => Cheapest | undefined,
): number {
  let total = 0;
  for (const t of TIERS)
    for (const line of b.tiers[t].lines)
      total +=
        Math.min(line.each, cheapestOf(line.card)?.each ?? line.each) *
        line.card.qty;
  return total;
}

/** A printing and finish of a card, and what one copy of it costs. */
export interface Cheapest {
  printing: PrintingOption;
  finish: Finish;
  each: number;
}

/**
 * The cheapest paper copy among a card's printings, in any finish: switching
 * to it is what "a cheaper version" can save. A digital or oversized printing
 * is not a copy anyone can play with.
 */
export function cheapest(
  printings: readonly PrintingOption[],
  currency: Currency,
): Cheapest | undefined {
  let best: Cheapest | undefined;
  for (const printing of printings) {
    if (printing.facts.digital === true || printing.facts.oversized === true)
      continue;
    for (const finish of printing.finishes) {
      const each = printing.prices[currency][finish];
      if (each !== undefined && (best === undefined || each < best.each))
        best = { printing, finish, each };
    }
  }
  return best;
}
