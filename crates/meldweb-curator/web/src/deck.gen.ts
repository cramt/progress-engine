// Generated from crates/meldweb-curator/wasm/src/lib.rs. Do not edit:
// UPDATE_TS=1 cargo test -p meldweb-wasm rewrites it.

export type ParsedCollection =
  | { kind: "collection"; places: Place[]; cards: OwnedCard[]; total: number }
  | { kind: "refused"; message: string };

/**
 * One line of the collection (ADR-0023).
 */
export interface OwnedCard {
  /**
   * Position in the file's `cards` list, 0-based: how an edit finds it.
   */
  index: number;
  card: CardRef;
  qty: number;
  finish: Finish;
  /**
   * The place the copies are in; absent for unsorted.
   */
  at?: string;
}

export type Finish = "nonfoil" | "foil" | "etched";

export type CardRef =
  | { kind: "printing"; set: string; num: string }
  | { kind: "name"; name: string };

export interface Place {
  name: string;
  /**
   * The path of the deck this place is, when it is one.
   */
  deck?: string;
}

export type Imported =
  | { kind: "imported"; toml: string; unreadable: Unreadable[] }
  | { kind: "refused"; message: string };

/**
 * A line of pasted Archidekt text the import could not carry over whole.
 */
export interface Unreadable {
  /**
   * 1-based, counting every line of the pasted text.
   */
  line: number;
  text: string;
  reason: string;
}

/**
 * A card to add: a [`CardRef`], and for a printing optionally the card's
 * name, written beside the new line as its comment the way an import does.
 */
export type NewCard =
  | { kind: "printing"; set: string; num: string; name?: string }
  | { kind: "name"; name: string };

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

export interface Category {
  name: string;
  /**
   * Absent for a label that says nothing about where a card is.
   */
  kind?: Kind;
}
