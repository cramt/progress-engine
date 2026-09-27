// Generated from crates/meldweb-curator/wasm/src/lib.rs. Do not edit:
// UPDATE_TS=1 cargo test -p meldweb-wasm rewrites it.

export type Parsed =
  | { kind: "deck"; name?: string; format?: string; categories: Category[]; cards: Card[]; total: number }
  | { kind: "refused"; message: string };

export interface Card {
  /**
   * Position in the file's `cards` list, 0-based: how an edit finds it.
   */
  index: number;
  card: CardRef;
  qty: number;
  finish: Finish;
  categories: string[];
  /**
   * The deepest type among its categories, `"in-deck"` when none is typed.
   */
  place: Kind;
  /**
   * Counted toward the deck: `place` is within in-deck.
   */
  inDeck: boolean;
}

/**
 * The category type tree (ADR-0020), as the strings the file uses.
 */
export type Kind = "in-deck" | "commander" | "not-in-deck" | "sideboard" | "companion" | "maybeboard" | "attractions" | "sticker-sheet";

export type Finish = "nonfoil" | "foil" | "etched";

export type CardRef =
  | { kind: "printing"; set: string; num: string }
  | { kind: "name"; name: string };

export interface Category {
  name: string;
  /**
   * Absent for a label that says nothing about where a card is.
   */
  kind?: Kind;
}
