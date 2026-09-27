import { useEffect, useState } from "react";
import type { Card } from "../deck";
import { fetchPrintings, type Printings, printingKey } from "../scryfall";

/**
 * The cards whose printing the page has neither found nor asked for: a card
 * an edit just added, by printing or by name. Each is asked about once; a
 * card Scryfall cannot find stays unfound rather than asked for again.
 */
export function missingCards(
  cards: readonly Card[],
  known: Printings,
  asked: ReadonlySet<string>,
): Card[] {
  const seen = new Set<string>();
  return cards.filter((c) => {
    const key = printingKey(c.card);
    if (known.has(key) || asked.has(key) || seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

/**
 * The editor's printings, growing with the deck: the ones loaded with it,
 * plus each card an edit adds, looked up through the same collection path
 * (and Scryfall queue) that loaded the rest.
 */
export function usePrintings(
  loaded: Printings,
  cards: readonly Card[],
): Printings {
  const [printings, setPrintings] = useState<Printings>(loaded);
  // The deck as loaded was asked about already, found or not.
  const [asked] = useState(
    () => new Set(cards.map((c) => printingKey(c.card))),
  );
  useEffect(() => {
    const missing = missingCards(cards, printings, asked);
    if (missing.length === 0) return;
    for (const c of missing) asked.add(printingKey(c.card));
    fetchPrintings(missing).then(
      (found) => {
        if (found.size > 0) setPrintings((now) => new Map([...now, ...found]));
      },
      () => {
        // The card shows by the name the file gives it, as when the deck
        // loaded without Scryfall.
      },
    );
  }, [cards, printings, asked]);
  return printings;
}

/**
 * `{ "set/num": name }` for every card the file names by printing and the
 * page knows the name of: what the Archidekt export needs, since the file
 * does not name those cards itself. Keyed as the file writes the printing.
 */
export function printingNames(
  cards: readonly Card[],
  printings: Printings,
): Record<string, string> {
  const names: Record<string, string> = {};
  for (const c of cards) {
    if (c.card.kind !== "printing") continue;
    const name = printings.get(printingKey(c.card))?.name;
    if (name !== undefined) names[`${c.card.set}/${c.card.num}`] = name;
  }
  return names;
}
