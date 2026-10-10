import { useEffect, useRef, useState } from "react";
import type { CardRef } from "../deck";
import { fetchPrintings, type Printings, printingKey } from "../scryfall";

/**
 * The printings of cards added since the page loaded, looked up as they
 * appear; a card Scryfall cannot find is asked about once.
 */
export function useGrowingPrintings(
  loaded: Printings,
  cards: readonly { card: CardRef }[],
): Printings {
  const [printings, setPrintings] = useState(loaded);
  const asked = useRef(new Set(loaded.keys()));
  useEffect(() => {
    const missing = cards.filter(
      (c) => !asked.current.has(printingKey(c.card)),
    );
    if (missing.length === 0) return;
    for (const c of missing) asked.current.add(printingKey(c.card));
    // Not aborted on cleanup: the keys are already marked asked, so an
    // aborted lookup would never be made again.
    fetchPrintings(missing)
      .then((found) => setPrintings((p) => new Map([...p, ...found])))
      .catch(() => {});
  }, [cards]);
  return printings;
}
