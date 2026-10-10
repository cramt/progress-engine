/**
 * The deck by what it costs: every card's price on its own printing beside
 * its cheapest, dearest first, so where the money goes and what a cheaper
 * printing saves read off one table. Where to proxy or ask around is the
 * reader's call; people draw that line in different places.
 */
import type { PrintingOption } from "../card/prints";
import type { Card, Finish } from "../deck";
import { type PriceBook, unitPrice } from "../prices";
import type { Currency } from "../scryfall";

export interface Priced {
  card: Card;
  /** One copy, in the line's own finish. */
  each: number;
}

export interface Breakdown {
  /** The deck's priced lines, by one copy's price, dearest first. */
  lines: Priced[];
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
  nameOf: (card: Card) => string,
): Breakdown {
  const lines: Priced[] = [];
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
    lines.push({ card, each });
    total += each * card.qty;
  }
  lines.sort(
    (a, b) => b.each - a.each || nameOf(a.card).localeCompare(nameOf(b.card)),
  );
  return { lines, unpriced, total, outside };
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
  for (const line of b.lines)
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
