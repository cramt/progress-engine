import type { Pin } from "../deck.gen";
import { pinned_printing } from "../wasm/pkg/meldweb_wasm.js";
import { rankPrintings } from "./preference";
import { fetchAllPrintings, type PrintingOption, printsByName } from "./prints";

/** Every printing of the card called `name`. */
export type LoadPrintings = (
  name: string,
) => Promise<readonly PrintingOption[]>;

const fromScryfall: LoadPrintings = (name) =>
  fetchAllPrintings(printsByName(name));

/**
 * The printing a card added to a deck by name gets: its pin in `settings`,
 * asked of no one, else the one `settings` ranks first. Null when Scryfall
 * has none. Which pin is the card's is Rust's to say, as the ranking is.
 */
export async function bestPrinting(
  name: string,
  settings: string | null,
  load: LoadPrintings = fromScryfall,
): Promise<{ set: string; num: string } | null> {
  const pin = pinned_printing(settings ?? undefined, name);
  if (pin !== undefined) {
    const { set, num } = JSON.parse(pin) as Pin;
    return { set, num };
  }
  const best = rankPrintings(settings, await load(name)).ranked[0];
  return best ? { set: best.option.set, num: best.option.num } : null;
}
