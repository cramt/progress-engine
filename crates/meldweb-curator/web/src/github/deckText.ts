/**
 * What this directory needs from `chip-decklist`, as one seam that tests can
 * replace.
 */
import {
  commitMessage,
  importArchidekt as importArchidektRaw,
  newDeck,
  type Parsed,
  parseDeck,
  setDeckMeta,
} from "../deck";

export type Imported =
  | {
      kind: "imported";
      toml: string;
      unreadable: { line: number; text: string; reason: string }[];
    }
  | { kind: "refused"; message: string };

export interface DeckText {
  parseDeck(text: string): Parsed;
  /** The deterministic changelog subject and body for one save (#124). */
  commitMessage(before: string, after: string, path: string): string;
  /** The text of an empty deck with `name` and `format` set. */
  newDeck(name: string, format: string): string;
  /** The text with `name` set, and `format` when given. */
  setDeckMeta(text: string, name: string, format?: string): string;
  importArchidekt(text: string): Imported;
}

/**
 * ADAPTER, to delete at merge: `importArchidekt` is changing from a string
 * that throws on refusal to the typed `Imported`. This takes either.
 */
function importArchidekt(text: string): Imported {
  try {
    const r = importArchidektRaw(text) as unknown;
    return typeof r === "string"
      ? { kind: "imported", toml: r, unreadable: [] }
      : (r as Imported);
  } catch (e) {
    return {
      kind: "refused",
      message: e instanceof Error ? e.message : String(e),
    };
  }
}

export const deckText: DeckText = {
  parseDeck,
  commitMessage,
  newDeck,
  setDeckMeta,
  importArchidekt,
};
