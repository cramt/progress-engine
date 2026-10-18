// Generated from crates/mirage-mockery/wasm/src/lib.rs. Do not edit:
// UPDATE_TS=1 cargo test -p mockery-wasm rewrites it.

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
 * A real card to compare with.
 */
export interface Real {
  name: string;
  oracle: string;
}

/**
 * A `.card.toml`, read.
 */
export type Checked =
  | {
      kind: "card";
      name: string;
      type_line: string;
      cost: string;
      word?: string;
      art?: string;
      lines: number;
      unread: Unread[];
    }
  | { kind: "refused"; message: string };

/**
 * A line of rules text Insight could not read, and the word it stopped at.
 */
export interface Unread {
  line: string;
  read: string;
  at: string;
}
