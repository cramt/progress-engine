// Generated from crates/mirage-mockery/wasm/src/lib.rs. Do not edit:
// UPDATE_TS=1 cargo test -p mockery-wasm rewrites it.

/**
 * A card file as a form, or why it cannot be one.
 */
export type Opened =
  | { kind: "form"; form: Form; parts: Parts }
  | { kind: "refused"; message: string };

/**
 * What a card file's parts are, by the table its second part is in: what
 * the form's face picker names them.
 */
export type Parts = "one" | "frontBack" | "halves" | "adventure" | "prepare";

/**
 * Every field of a card file. An empty string is a field the file leaves out.
 */
export interface Form {
  name: string;
  cost: string;
  typeLine: string;
  rules: string;
  flavor: string;
  corner: FormCorner;
  /**
   * The colour indicator's letters in WUBRG, like `"WR"`.
   */
  colorIndicator: string;
  art: string;
  artist: string;
}

/**
 * The bottom-right corner, one of them at most, as [`crate::card::Corner`].
 * Power and toughness are apart here, as the form asks for them.
 */
export type FormCorner =
  | { kind: "none" }
  | { kind: "pt"; power: string; toughness: string }
  | { kind: "loyalty"; value: string }
  | { kind: "defense"; value: string };

/**
 * One card on a sheet: its text, and its art's bytes when it names some.
 */
export interface SheetCard {
  text: string;
  qty: number;
}

/**
 * The same parameters as a proxy sheet, so the two print alike.
 */
export interface CardSheet {
  paper: SheetPaper;
  gapMm: number;
  marginMm: number;
  cutMarks: boolean;
}

/**
 * Portrait, as Phyrexian Metamorph's paper.
 */
export type SheetPaper = "a4" | "a3" | "letter" | "legal";

/**
 * A `.card.toml`, read.
 */
export type Checked =
  | { kind: "card"; name: string; type_line: string; arts: string[]; faces: number; lines: number; unread: Unread[] }
  | { kind: "refused"; message: string };

/**
 * A line of rules text Insight could not read, and the word it stopped at.
 */
export interface Unread {
  line: string;
  read: string;
  at: string;
}
