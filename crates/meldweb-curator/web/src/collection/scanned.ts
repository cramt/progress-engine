import { addOwned, parseCollection, setOwnedQty } from "../collection";
import type { Finish, NewCard } from "../deck";
import { printingKey } from "../scryfall";

/**
 * A copy a scan put in the collection: the card, by the printing the scanner
 * guessed or by name alone, and where it went.
 */
export interface ScannedCopy {
  card: NewCard;
  at: string | null;
  finish: Finish;
}

/** The collection with the scanned copy in it, on a line alike if there is one. */
export function addScanned(text: string, copy: ScannedCopy): string {
  return addOwned(text, copy.card, copy.at, 1, copy.finish);
}

/**
 * The collection with one copy fewer on the line a scan put it on, for taking
 * back a misread. Throws when no line holds it any more: it was moved or
 * removed since, and which copy to take is then the user's call.
 */
export function removeScanned(text: string, copy: ScannedCopy): string {
  const parsed = parseCollection(text);
  if (parsed.kind !== "collection") throw new Error(parsed.message);
  const key = printingKey(copy.card);
  const line = parsed.cards.find(
    (c) =>
      (c.at ?? null) === copy.at &&
      c.finish === copy.finish &&
      printingKey(c.card) === key,
  );
  if (!line) {
    throw new Error(
      "that copy is no longer where the scan put it, so it was left as it is",
    );
  }
  return setOwnedQty(text, line.index, line.qty - 1);
}

/**
 * The collection with the scanned copy moved from where the scan put it to
 * `to`. Throws, changing nothing, when it is no longer there.
 */
export function moveScanned(
  text: string,
  copy: ScannedCopy,
  to: string | null,
): string {
  return addScanned(removeScanned(text, copy), { ...copy, at: to });
}
