// Generated from crates/meldweb-curator/wasm/src/lib.rs. Do not edit:
// UPDATE_TS=1 cargo test -p meldweb-wasm rewrites it.

export type Parsed =
  | { kind: "deck"; entries: Entry[]; total: number }
  | { kind: "refused"; line: number; text: string; message: string };

export interface Entry {
  /**
   * 1-based line in the text it was parsed from: how an edit finds it.
   */
  line: number;
  qty: number;
  name: string;
  set?: string;
  num?: string;
  foil: boolean;
  categories: Category[];
  /**
   * Decided here rather than in TypeScript, for the same reason the parser is.
   */
  commander: boolean;
  /**
   * Sideboard, maybeboard, companion or `{noDeck}`: listed, not among the deck.
   */
  outside: boolean;
}

/**
 * One `[Category{flag}]` element. Archidekt allows several per line,
 * comma-separated: `[Big Colorless,Test]`.
 */
export interface Category {
  /**
   * Category text with any `{flags}` stripped, e.g. `Commander`.
   */
  name: string;
  /**
   * Flags inside braces, lowercased, e.g. `top`, `nodeck`.
   */
  flags: string[];
}
