import type { Imported, Kind, Parsed } from "./deck.gen";
import init, {
  declare_category,
  export_archidekt,
  import_archidekt,
  initSync,
  parse_deck,
  set_card_categories,
} from "./wasm/pkg/meldweb_wasm.js";

export type {
  Card,
  CardRef,
  Category,
  Finish,
  Imported,
  Kind,
  Parsed,
  Unreadable,
} from "./deck.gen";

/** Loads the parser. Everything else in this module needs it done first. */
export async function loadDeck(): Promise<void> {
  await init();
}

/** For Node, which has no `fetch` for a file URL: hand over the bytes. */
export function loadDeckSync(module: BufferSource): void {
  initSync({ module });
}

/**
 * The shape is generated from the Rust side, and meldweb-wasm's tests pin the
 * JSON to it, so this cast is the one place the two are trusted to agree.
 */
export function parseDeck(text: string): Parsed {
  return JSON.parse(parse_deck(text)) as Parsed;
}

/**
 * Pasted Archidekt text as a `.deck.toml`, read the way Archidekt reads it,
 * with every line that could not be carried over and why. `refused` when no
 * line was a card.
 */
export function importArchidekt(text: string): Imported {
  return JSON.parse(import_archidekt(text)) as Imported;
}

/**
 * The deck as Archidekt text, names only. The file names some cards only by
 * printing, so `names` maps each `"set/num"` to the card's name from the
 * Scryfall data the page already has. Throws, listing them, when a printing
 * has no name.
 */
export function exportArchidekt(
  text: string,
  names: Readonly<Record<string, string>> = {},
): string {
  return export_archidekt(text, JSON.stringify(names));
}

/**
 * The deck text with one card's categories replaced and the rest of the file
 * untouched. Throws when the result is a deck the format does not allow, such
 * as a card in two places at once.
 */
export function setCardCategories(
  text: string,
  index: number,
  categories: readonly string[],
): string {
  return set_card_categories(text, index, JSON.stringify(categories));
}

/** The deck text with `name` declared, typed or not. */
export function declareCategory(
  text: string,
  name: string,
  kind?: Kind,
): string {
  return declare_category(text, name, kind ?? "");
}
