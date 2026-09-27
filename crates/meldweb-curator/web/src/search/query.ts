import type { Card } from "../deck";
import { type Printings, printingKey } from "../scryfall";

const WUBRG = ["W", "U", "B", "R", "G"] as const;

/**
 * The deck's colour identity, from its commanders' Scryfall data: their
 * identities together in WUBRG order, `"C"` when that is no colour at all.
 * `null` when there is no commander, or one Scryfall has not told us about,
 * since then any identity we wrote would be a guess.
 */
export function deckIdentity(
  cards: readonly Card[],
  printings: Printings,
): string | null {
  const commanders = cards.filter((c) => c.place === "commander");
  if (commanders.length === 0) return null;
  const colours = new Set<string>();
  for (const c of commanders) {
    const printing = printings.get(printingKey(c.card));
    if (!printing) return null;
    for (const x of printing.colorIdentity) colours.add(x.toUpperCase());
  }
  const id = WUBRG.filter((x) => colours.has(x)).join("");
  return id === "" ? "C" : id;
}

/** What the smart filters append; each is left out when the deck has none. */
export interface SmartFilters {
  identity: string | null;
  format: string | null;
}

/** The smart filters as the query text they add, e.g. `id<=UG f:commander`. */
export function filterText({ identity, format }: SmartFilters): string {
  return [
    identity === null ? null : `id<=${identity}`,
    format === null ? null : `f:${format}`,
  ]
    .filter((x) => x !== null)
    .join(" ");
}

/**
 * The query Scryfall is sent. With smart filters on, the user's query is
 * wrapped in parentheses first, because `or` binds looser than the implicit
 * `and`: `o:draw or o:scry id<=WG` would filter only `o:scry`.
 */
export function composeQuery(
  user: string,
  filters: SmartFilters | null,
): string {
  const query = user.trim();
  if (query === "" || filters === null) return query;
  const extra = filterText(filters);
  return extra === "" ? query : `(${query}) ${extra}`;
}
