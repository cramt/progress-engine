/**
 * What this directory needs from `chip-decklist`, as one seam that tests can
 * replace.
 */
import {
  declareCategory,
  type Imported,
  importArchidekt,
  newDeck,
  type Parsed,
  parseDeck,
  setCardCategories,
  setCardPrinting,
  setDeckMeta,
  setVariantOf,
} from "../deck";

export type { Imported };

export interface DeckText {
  parseDeck(text: string): Parsed;
  /** The text of an empty deck with `name` and `format` set. */
  newDeck(name: string, format: string): string;
  /** The text with `name` set, and `format` when given. */
  setDeckMeta(text: string, name: string, format?: string): string;
  /** The text as a variant of the deck at `parent`, or alone for `null`. */
  setVariantOf(text: string, parent: string | null): string;
  importArchidekt(text: string): Imported;
  /** The text with card `index` named by the printing `set/num`. */
  setCardPrinting(
    text: string,
    index: number,
    set: string,
    num: string,
  ): string;
  /** The text with `name` declared, typed or not. */
  declareCategory(text: string, name: string): string;
  /** The text with card `index`'s categories replaced. */
  setCardCategories(
    text: string,
    index: number,
    categories: readonly string[],
  ): string;
}

export const deckText: DeckText = {
  parseDeck,
  newDeck,
  setDeckMeta,
  setVariantOf,
  importArchidekt,
  setCardPrinting,
  declareCategory,
  setCardCategories,
};
