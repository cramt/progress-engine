/**
 * What this directory needs from `chip-decklist`, as one seam.
 *
 * `commitMessage` and `newDeck` are being added to `../deck` concurrently, and
 * `importArchidekt` is changing to return a typed result (the shared brief's
 * wasm contract). Until those land the namespace is read through a cast, so a
 * missing function fails when called rather than failing the type check. Once
 * they are in, this becomes plain named imports.
 */

import type { Parsed } from "../deck";
import * as deck from "../deck";

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
  importArchidekt(text: string): Imported;
}

export const deckText: DeckText = deck as unknown as DeckText;
