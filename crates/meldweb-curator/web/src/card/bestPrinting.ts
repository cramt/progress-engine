import type { Card } from "../deck";
import type { Pin } from "../deck.gen";
import { rankPrintings } from "./preference";
import { fetchAllPrintings, type PrintingOption, printsByName } from "./prints";

/** Every printing of the card called `name`. */
export type LoadPrintings = (
  name: string,
) => Promise<readonly PrintingOption[]>;

const fromScryfall: LoadPrintings = (name) =>
  fetchAllPrintings(printsByName(name));

/**
 * The printing a card added to a deck by name gets: its pin, asked of no one,
 * else the one `settings` ranks first. Null when Scryfall has none.
 */
export async function bestPrinting(
  name: string,
  settings: string | null,
  pins: readonly Pin[],
  load: LoadPrintings = fromScryfall,
): Promise<{ set: string; num: string } | null> {
  const pin = pins.find((p) => p.name.toLowerCase() === name.toLowerCase());
  if (pin) return { set: pin.set, num: pin.num };
  const best = rankPrintings(settings, await load(name)).ranked[0];
  return best ? { set: best.option.set, num: best.option.num } : null;
}

/**
 * The line an add made by name, when it made one rather than adding a copy
 * to a line the deck had: the name-only line for `name` that `before` lacks.
 */
export function addedLine(
  before: readonly Card[],
  after: readonly Card[],
  name: string,
): number | null {
  if (after.length <= before.length) return null;
  const line = after.findLast(
    (c) =>
      c.card.kind === "name" &&
      c.card.name.toLowerCase() === name.toLowerCase(),
  );
  return line ? line.index : null;
}
