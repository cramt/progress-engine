/**
 * The collection's side of `chip-decklist` (ADR-0023): what is owned and in
 * which place, parsed and edited in Rust as a deck is. Load `deck.ts` first;
 * this module shares its wasm.
 */
import type { Finish, NewCard, ParsedCollection } from "./deck.gen";
import {
  collection_add,
  collection_commit_message,
  collection_move,
  collection_move_lines,
  collection_reprint,
  collection_set_qty,
  declare_place,
  parse_collection,
  rename_place,
  undeclare_place,
} from "./wasm/pkg/meldweb_wasm.js";

export type { OwnedCard, ParsedCollection, Place } from "./deck.gen";

/** As `parseDeck`: the one cast, trusted because the Rust tests pin it. */
export function parseCollection(text: string): ParsedCollection {
  return JSON.parse(parse_collection(text)) as ParsedCollection;
}

/*
 * Each edit takes the collection's text and returns the next, changing only
 * the lines it is about, and throws when the result is not a collection the
 * format allows. `index` is an `OwnedCard.index`; a place of `null` is
 * unsorted.
 */

/**
 * `qty` more of `card`: more on the line already holding it alike in that
 * place, or else a new last line. A printing may carry its `name`, written
 * beside the line as a comment.
 */
export function addOwned(
  text: string,
  card: NewCard,
  at: string | null,
  qty = 1,
  finish: Finish = "nonfoil",
): string {
  return collection_add(text, JSON.stringify(card), qty, finish, at ?? "");
}

/**
 * `qty` of a line's copies moved to `to`. All of them moves the line; part
 * leaves the rest where it was. Either way they join a line already holding
 * the card alike there.
 */
export function moveOwned(
  text: string,
  index: number,
  qty: number,
  to: string | null,
): string {
  return collection_move(text, index, qty, to ?? "");
}

/**
 * All of each line in `indices` moved to `to`, as one edit, each joining a
 * line already holding the card alike there.
 */
export function moveOwnedLines(
  text: string,
  indices: readonly number[],
  to: string | null,
): string {
  return collection_move_lines(text, Uint32Array.from(indices), to ?? "");
}

/**
 * `qty` of a line's copies made `printing` (or left the card they are, for
 * null) in `finish`, where they are. All of a line changes in place, part
 * leaves the rest; either way they join a line already holding them alike.
 */
export function reprintOwned(
  text: string,
  index: number,
  qty: number,
  printing: { set: string; num: string } | null,
  finish: Finish,
): string {
  return collection_reprint(
    text,
    index,
    qty,
    printing?.set ?? "",
    printing?.num ?? "",
    finish,
  );
}

/** A line at `qty` copies; 0 removes it. */
export function setOwnedQty(text: string, index: number, qty: number): string {
  return collection_set_qty(text, index, qty);
}

/** Declares a place, standing for the deck at `deck` when one is given. */
export function declarePlace(
  text: string,
  name: string,
  deck?: string,
): string {
  return declare_place(text, name, deck ?? "");
}

/** Drops a place, which must hold nothing. */
export function undeclarePlace(text: string, name: string): string {
  return undeclare_place(text, name);
}

/** The place `from` called `to`, its cards with it. */
export function renamePlace(text: string, from: string, to: string): string {
  return rename_place(text, from, to);
}

/** A line of the collection's changelog; `before` is `""` for the first save. */
export function collectionCommitMessage(
  before: string,
  after: string,
  path: string,
): string {
  return collection_commit_message(before, after, path);
}
