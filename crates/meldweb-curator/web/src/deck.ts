import type { Kind, Parsed } from "./deck.gen";
import init, {
  declare_category,
  import_archidekt,
  initSync,
  parse_deck,
  set_card_categories,
} from "./wasm/pkg/meldweb_wasm.js";

export type { Card, CardRef, Category, Finish, Kind, Parsed } from "./deck.gen";

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

/** Archidekt's text export as a `.deck.toml`. Throws what the import refuses. */
export function importArchidekt(text: string): string {
  return import_archidekt(text);
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
