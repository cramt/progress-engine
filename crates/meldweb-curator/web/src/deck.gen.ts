// Generated from crates/meldweb-curator/wasm/src/lib.rs. Do not edit:
// UPDATE_TS=1 cargo test -p meldweb-wasm rewrites it.

/**
 * `meldweb.toml` as the settings page edits it: its rules, each query unread
 * so a bad one can be fixed in place, and the default rules beside them.
 */
export type SettingsRules =
  | { kind: "read"; pins: Pin[]; rules: RuleText[]; decks: string[]; declared: boolean; defaults: RuleText[] }
  | { kind: "refused"; message: string; defaults: RuleText[] };

/**
 * One rule, as the file wrote it.
 */
export interface RuleText {
  verb: Verb;
  query: string;
}

export type Verb = "prefer" | "avoid";

/**
 * One card's own printing: a `prefer` rule naming exactly that card and
 * printing, `!"Sol Ring" set:c21 cn:263`. It is a rule like any other to the
 * ranking, written above the rest so it beats them all for its card; the
 * settings page shows it as the card it is, not as a query.
 */
export interface Pin {
  name: string;
  set: string;
  num: string;
}

export type Ranked =
  | { kind: "ranked"; rules: RuleText[]; declared: boolean; pins: number[]; order: RankedPrinting[] }
  | { kind: "refused"; message: string };

/**
 * One printing in [`Ranked`]'s order: its index in what was ranked, and
 * which rules it matched.
 */
export interface RankedPrinting {
  index: number;
  matched: number[];
}

export type Compared =
  | { kind: "diff"; changes: DeckChange[] }
  | { kind: "refused"; message: string };

/**
 * One change between two decks, as `chip_decklist::diff` finds it. A card
 * line is named by its index in the `before` deck, the `after` deck, or
 * both; `text` is the line a commit message says the change with.
 */
export type DeckChange =
  | { kind: "add"; after: number; text: string }
  | { kind: "remove"; before: number; text: string }
  | { kind: "qty"; before: number; after: number; text: string }
  | { kind: "move"; before: number; after: number; text: string }
  | { kind: "printing"; before: number; after: number; text: string }
  | { kind: "finish"; before: number; after: number; text: string }
  | { kind: "declare"; category: string; text: string }
  | { kind: "undeclare"; category: string; text: string }
  | { kind: "retype"; category: string; text: string }
  | { kind: "rename"; text: string }
  | { kind: "format"; text: string }
  | { kind: "variantOf"; text: string }
  | { kind: "cover"; text: string }
  | { kind: "description"; text: string };

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
  | { kind: "imported"; toml: string; unreadable: Unreadable[]; setOnly: SetOnly[] }
  | { kind: "refused"; message: string };

/**
 * A card named by name whose Archidekt line also gave a set.
 */
export interface SetOnly {
  /**
   * Into the deck's cards, as the edits index them.
   */
  index: number;
  line: number;
  text: string;
  name: string;
  set: string;
}

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
 * What an add did: the next text, the line holding the card now, and
 * whether that line is new.
 */
export interface Added {
  text: string;
  line: number;
  made: boolean;
}

/**
 * Where a card added to a deck goes.
 */
export type AddTo =
  | { kind: "automatic" }
  | { kind: "categories"; categories: string[] };

/**
 * A card to add: a [`CardRef`], and for a printing optionally the card's
 * name, written beside the new line as its comment the way an import does.
 */
export type NewCard =
  | { kind: "printing"; set: string; num: string; name?: string }
  | { kind: "name"; name: string };

export type Parsed =
  | { kind: "deck"; name?: string; format?: string; variantOf?: string; cover?: Printing; description?: string; categories: Category[]; cards: Card[]; total: number }
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

/**
 * A printing, `set/num` in the file.
 */
export interface Printing {
  set: string;
  num: string;
}
