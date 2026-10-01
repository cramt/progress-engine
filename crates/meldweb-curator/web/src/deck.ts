import type { Finish, Imported, Kind, NewCard, Parsed } from "./deck.gen";
import init, {
  add_card,
  commit_message,
  declare_category,
  export_archidekt,
  import_archidekt,
  initSync,
  new_deck,
  parse_deck,
  remove_card,
  set_card_categories,
  set_card_finish,
  set_card_printing,
  set_card_qty,
  set_commander,
  set_deck_cover,
  set_deck_description,
  set_deck_meta,
} from "./wasm/pkg/meldweb_wasm.js";

export type {
  Card,
  CardRef,
  Category,
  Finish,
  Imported,
  Kind,
  NewCard,
  Parsed,
  SetOnly,
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

/*
 * Every edit below takes the deck text and returns the next text, with only
 * the lines the edit is about changed. Each throws when the result is a deck
 * the format does not allow, such as a card in two places at once or in a
 * category never declared. `index` is a card's `Card.index`.
 */

/** The deck text with one card's categories replaced. */
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

/** The deck text with a card at `qty` copies; 0 removes it. */
export function setCardQty(text: string, index: number, qty: number): string {
  return set_card_qty(text, index, qty);
}

/** The deck text without a card's line. */
export function removeCard(text: string, index: number): string {
  return remove_card(text, index);
}

/**
 * The deck text with a card as a commander: it joins the deck's
 * commander-typed category, first, declaring `Commander` if there is none,
 * and leaves any category that put it outside the deck.
 */
export function setCommander(text: string, index: number): string {
  return set_commander(text, index);
}

/** The deck text with a card named by the printing `set/num`. */
export function setCardPrinting(
  text: string,
  index: number,
  set: string,
  num: string,
): string {
  return set_card_printing(text, index, set, num);
}

/** The deck text with a card's finish changed. */
export function setCardFinish(
  text: string,
  index: number,
  finish: Finish,
): string {
  return set_card_finish(text, index, finish);
}

/**
 * The deck text with one more `card` in `categories`: a new last line, or
 * one more of the card already there in exactly those categories. A printing
 * may carry its `name`, written beside the line as a comment so the commit
 * message can say it.
 */
export function addCard(
  text: string,
  card: NewCard,
  categories: readonly string[] = [],
): string {
  return add_card(text, JSON.stringify(card), JSON.stringify(categories));
}

/**
 * The commit message for saving `before` as `after` at `path`, a line of the
 * deck's changelog. `before` is `""` for a deck's first save.
 */
export function commitMessage(
  before: string,
  after: string,
  path: string,
): string {
  return commit_message(before, after, path);
}

/**
 * The deck text with its `name` set, and its `format` when one is given; a
 * file without them, as an Archidekt import is, gains them at its top.
 */
export function setDeckMeta(
  text: string,
  name: string,
  format?: string,
): string {
  return set_deck_meta(text, name, format ?? "");
}

/** The deck text with its `cover` set to the printing, or dropped for `null`. */
export function setDeckCover(
  text: string,
  cover: { set: string; num: string } | null,
): string {
  return set_deck_cover(text, cover && `${cover.set}/${cover.num}`);
}

/** The deck text with its Markdown `description` set, or dropped for blank. */
export function setDeckDescription(text: string, description: string): string {
  return set_deck_description(text, description);
}

/** The text of a new, empty deck. An empty `format` is left out. */
export function newDeck(name: string, format: string): string {
  return new_deck(name, format);
}
