import type { OwnedCard } from "../collection";
import type { CardRef, Finish } from "../deck";
import { type CardPrices, type Currency, printingKey } from "../scryfall";

/** Today's prices, keyed as printings are; see `fetchPrices`. */
export type PriceBook = ReadonlyMap<string, CardPrices>;

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
  cards: readonly OwnedCard[],
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

export function formatPrice(amount: number, currency: Currency): string {
  return new Intl.NumberFormat(undefined, {
    style: "currency",
    currency: currency.toUpperCase(),
  }).format(amount);
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
