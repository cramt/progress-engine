import { useEffect, useRef, useState } from "react";
import type { CardRef, Finish } from "./deck";
import {
  type CardPrices,
  type Currency,
  fetchPrices,
  printingKey,
} from "./scryfall";

/** Today's prices, keyed as printings are; see `fetchPrices`. */
export type PriceBook = ReadonlyMap<string, CardPrices>;

/** Some copies of one printing in one finish: a deck's line or a collection's. */
export interface Copies {
  card: CardRef;
  qty: number;
  finish: Finish;
}

/**
 * Today's price of every card, looked up after the page has opened, so a slow
 * or failed lookup costs the prices and nothing else.
 */
export function usePrices(cards: readonly { card: CardRef }[]): PriceBook {
  const [prices, setPrices] = useState<PriceBook>(new Map());
  const asked = useRef(new Set<string>());
  useEffect(() => {
    const missing = cards.filter(
      (c) => !asked.current.has(printingKey(c.card)),
    );
    if (missing.length === 0) return;
    for (const c of missing) asked.current.add(printingKey(c.card));
    fetchPrices(missing)
      .then((found) => setPrices((p) => new Map([...p, ...found])))
      .catch(() => {});
  }, [cards]);
  return prices;
}

/**
 * What one copy of a line sells for in `currency`, in its own finish: a foil
 * is never priced as a nonfoil, so a finish Scryfall has no price for is
 * unpriced rather than guessed.
 */
export function unitPrice(
  card: { card: CardRef; finish: Finish },
  prices: PriceBook,
  currency: Currency,
): number | undefined {
  return prices.get(printingKey(card.card))?.[currency][card.finish];
}

export interface Worth {
  total: number;
  /** Copies with no price, which the total leaves out. */
  unpriced: number;
}

export function worth(
  cards: readonly Copies[],
  prices: PriceBook,
  currency: Currency,
): Worth {
  let total = 0;
  let unpriced = 0;
  for (const c of cards) {
    const each = unitPrice(c, prices, currency);
    if (each === undefined) unpriced += c.qty;
    else total += each * c.qty;
  }
  return { total, unpriced };
}

export const CURRENCIES: readonly Currency[] = ["eur", "usd"];

// A formatter is slow to make and every price row asks for one, so each
// currency's is made once.
const formats = new Map<Currency, Intl.NumberFormat>();

export function formatPrice(amount: number, currency: Currency): string {
  let format = formats.get(currency);
  if (!format) {
    format = new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: currency.toUpperCase(),
    });
    formats.set(currency, format);
  }
  return format.format(amount);
}

const KEY = "meldweb.currency";

/** The currency last picked in this browser; euros until one is. */
export function loadCurrency(
  storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage,
): Currency {
  try {
    const saved = storage?.getItem(KEY);
    return CURRENCIES.find((c) => c === saved) ?? "eur";
  } catch {
    return "eur";
  }
}

export function saveCurrency(
  currency: Currency,
  storage: Pick<Storage, "setItem"> | undefined = globalThis.localStorage,
): void {
  try {
    storage?.setItem(KEY, currency);
  } catch {
    // blocked storage: the pick lasts as long as the page
  }
}
