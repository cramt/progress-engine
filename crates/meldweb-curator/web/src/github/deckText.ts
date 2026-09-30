/**
 * What this directory needs from `chip-decklist`, as one seam that tests can
 * replace.
 */
import {
  commitMessage,
  declareCategory,
  type Imported,
  importArchidekt,
  newDeck,
  type Parsed,
  parseDeck,
  setCardCategories,
  setCardPrinting,
  setDeckMeta,
} from "../deck";

export type { Imported };

export interface DeckText {
  parseDeck(text: string): Parsed;
  /** The deterministic changelog subject and body for one save (#124). */
  commitMessage(before: string, after: string, path: string): string;
  /** The text of an empty deck with `name` and `format` set. */
  newDeck(name: string, format: string): string;
  /** The text with `name` set, and `format` when given. */
  setDeckMeta(text: string, name: string, format?: string): string;
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
  commitMessage,
  newDeck,
  setDeckMeta,
  importArchidekt,
  setCardPrinting,
  declareCategory,
  setCardCategories,
};
