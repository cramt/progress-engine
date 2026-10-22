/**
 * Blame per card (ADR-0041): for each card, the commit that last changed it
 * and the one it came into the deck at, worked out in Rust (`blame.rs`) and
 * only said here. A card's change is its quantity, categories, printing or
 * finish; the deck's name or description changes no card.
 */
import type { Blame, BlameCommit, CardBlame } from "../deck.gen";

/** Where a card's change happened: a commit, or edits not saved yet. */
export type Touch =
  | { kind: "unsaved" }
  | { kind: "commit"; commit: BlameCommit };

const UNSAVED: Touch = { kind: "unsaved" };
const at = (commit: BlameCommit): Touch => ({ kind: "commit", commit });

/** What last changed the card. */
export function lastTouch(blame: Blame): Touch {
  switch (blame.kind) {
    case "addedUnsaved":
    case "changedUnsaved":
      return UNSAVED;
    case "added":
      return at(blame.added);
    case "changed":
      return at(blame.changed);
  }
}

/** Where the card came into the deck, in its current run. */
export function addedTouch(blame: Blame): Touch {
  return blame.kind === "addedUnsaved" ? UNSAVED : at(blame.added);
}

/** Whether the card is as it came in: its last change was its add. */
export function unchangedSinceAdded(blame: Blame): boolean {
  return blame.kind === "added" || blame.kind === "addedUnsaved";
}

/** A commit's day, short: `3 Oct`, with the year when it is not this one's. */
export function shortDay(seconds: number, now: Date): string {
  const d = new Date(seconds * 1000);
  return d.toLocaleDateString("en-GB", {
    day: "numeric",
    month: "short",
    ...(d.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
  });
}

/** A commit message's first line, as the log shows it. */
export function firstLine(message: string): string {
  return message.split("\n", 1)[0] ?? "";
}

/** How the Blame view orders its rows. */
export type BlameOrder = "changed" | "added" | "name";

/**
 * Sorts by `order`, newest first for a date and A to Z for a name. Unsaved
 * edits are the newest of all, and ties keep the deck's order.
 */
export function sortBlame<T extends { blame: CardBlame; name: string }>(
  rows: readonly T[],
  order: BlameOrder,
): T[] {
  const time = (t: Touch) =>
    t.kind === "unsaved" ? Number.POSITIVE_INFINITY : t.commit.authoredAt;
  const key = (r: T) =>
    order === "changed"
      ? time(lastTouch(r.blame.blame))
      : time(addedTouch(r.blame.blame));
  return [...rows].sort((a, b) =>
    order === "name"
      ? a.name.localeCompare(b.name) || a.blame.index - b.blame.index
      : key(b) - key(a) || a.blame.index - b.blame.index,
  );
}
