import type { CardRef } from "../deck";
import { type Printings, printingKey } from "../scryfall";

/**
 * `{ "set/num": name }` for every card the file names by printing and the
 * page knows the name of: what the Archidekt export needs, since the file
 * does not name those cards itself. Keyed as the file writes the printing.
 */
export function printingNames(
  cards: readonly { card: CardRef }[],
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
