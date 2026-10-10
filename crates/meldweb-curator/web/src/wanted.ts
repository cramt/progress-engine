/**
 * The wanted list's side of `chip-decklist` (ADR-0034): the cards wanted by
 * hand, and the ones the decks hold that the collection is short of, both
 * worked out in Rust. Load `deck.ts` first; this module shares its wasm.
 */
import type {
  Added,
  DeckCopies,
  DeckFile,
  Finish,
  NewCard,
  ParsedWanted,
} from "./deck.gen";
import {
  read_wanted,
  wanted_add,
  wanted_commit_message,
  wanted_set_deck_copies,
  wanted_set_finish,
  wanted_set_qty,
} from "./wasm/pkg/meldweb_wasm.js";

export type {
  DeckCopies,
  DeckFile,
  MissingCard,
  ParsedWanted,
  WantedCard,
} from "./deck.gen";

/**
 * The wanted list `text`, each want with how many the collection holds, and
 * what `decks` hold that `collection` is short of. `names` names the
 * printings the three files hold, as for an add.
 */
export function readWanted(
  text: string,
  collection: string,
  decks: readonly DeckFile[],
  names: Readonly<Record<string, string>> = {},
): ParsedWanted {
  return JSON.parse(
    read_wanted(text, collection, JSON.stringify(decks), JSON.stringify(names)),
  ) as ParsedWanted;
}

/** `qty` more of `card`, on the line already wanting it so or a new one. */
export function addWanted(
  text: string,
  card: NewCard,
  qty = 1,
  finish: Finish = "nonfoil",
  names: Readonly<Record<string, string>> = {},
): Added {
  return JSON.parse(
    wanted_add(text, JSON.stringify(card), qty, finish, JSON.stringify(names)),
  ) as Added;
}

/** A want at `qty` copies; 0 removes it. */
export function setWantedQty(text: string, index: number, qty: number): string {
  return wanted_set_qty(text, index, qty);
}

export function setWantedFinish(
  text: string,
  index: number,
  finish: Finish,
): string {
  return wanted_set_finish(text, index, finish);
}

/** Whether each deck needs its own copies or one set moves between them. */
export function setDeckCopies(text: string, copies: DeckCopies): string {
  return wanted_set_deck_copies(text, copies);
}

/** A line of the wanted list's changelog; `before` is `""` for the first save. */
export function wantedCommitMessage(
  before: string,
  after: string,
  path: string,
): string {
  return wanted_commit_message(before, after, path);
}
