import type { DeckEntry } from "../github/decks";
import { readSettings } from "../settings/rules";
import { order_decks } from "../wasm/pkg/meldweb_wasm.js";

/**
 * Decks in the order `meldweb.toml` declares, by stem, and those it does not
 * name after them by name; each deck's forks straight after it, so a budget
 * build sits beside the deck it came from (ADR-0028). Which deck a fork came
 * from is its history's say (ADR-0041); a fork whose parent is gone stands
 * on its own.
 */
export function familyOrder(
  decks: readonly DeckEntry[],
  order: readonly string[] = [],
): DeckEntry[] {
  const rank = new Map(order.map((stem, i) => [stem, i]));
  const ordered = [...decks].sort(
    (a, b) =>
      (rank.get(a.stem) ?? order.length) - (rank.get(b.stem) ?? order.length) ||
      a.name.localeCompare(b.name),
  );
  const stems = new Set(decks.map((d) => d.stem));
  const childrenOf = new Map<string, DeckEntry[]>();
  for (const d of ordered) {
    if (d.parent && d.parent !== d.stem && stems.has(d.parent)) {
      childrenOf.set(d.parent, [...(childrenOf.get(d.parent) ?? []), d]);
    }
  }
  const placed = new Set<string>();
  const out: DeckEntry[] = [];
  const place = (d: DeckEntry) => {
    // A cycle cannot come from history, but; each deck is placed once.
    if (placed.has(d.stem)) return;
    placed.add(d.stem);
    out.push(d);
    for (const child of childrenOf.get(d.stem) ?? []) place(child);
  };
  for (const d of ordered) {
    const parented = d.parent && childrenOf.get(d.parent)?.includes(d);
    if (!parented) place(d);
  }
  // What a cycle left unplaced, in order.
  for (const d of ordered) place(d);
  return out;
}

/**
 * The deck list's stems after dragging `from` onto `to`, before or after it,
 * from `shown`, the list as `familyOrder` laid it out. A deck takes its
 * forks with it and moves among its siblings: dropped on a deck of another
 * family, it goes beside that family. Null for a drop that moves nothing.
 */
export function moveDeck(
  shown: readonly DeckEntry[],
  from: string,
  to: string,
  side: "before" | "after",
): string[] | null {
  const parentOf = parents(shown);
  const level = parentOf.get(from) ?? null;
  // `to`, or the ancestor of it that is one of `from`'s siblings.
  let target: string | undefined = to;
  const seen = new Set<string>();
  while (target !== undefined && (parentOf.get(target) ?? null) !== level) {
    if (seen.has(target)) return null;
    seen.add(target);
    target = parentOf.get(target);
  }
  if (target === undefined || target === from) return null;
  const block = familyOf(shown, from);
  const rest = shown.filter((d) => !block.has(d.stem));
  const family = familyOf(rest, target);
  let at = rest.findIndex((d) => d.stem === target);
  if (at < 0) return null;
  if (side === "after") {
    while (family.has(rest[at + 1]?.stem ?? "")) at++;
    at++;
  }
  const out = [
    ...rest.slice(0, at),
    ...shown.filter((d) => block.has(d.stem)),
    ...rest.slice(at),
  ].map((d) => d.stem);
  return out.every((p, i) => p === shown[i]?.stem) ? null : out;
}

/**
 * The deck list's stems with `stem` one place earlier (`-1`) or later (`1`)
 * among its siblings, as Alt+arrow and the tile's menu move it. Null at an end.
 */
export function nudgeDeck(
  shown: readonly DeckEntry[],
  stem: string,
  by: -1 | 1,
): string[] | null {
  const parentOf = parents(shown);
  const level = parentOf.get(stem);
  const siblings = shown.filter((d) => parentOf.get(d.stem) === level);
  const at = siblings.findIndex((d) => d.stem === stem);
  const next = siblings[at + by];
  if (at < 0 || !next) return null;
  return moveDeck(shown, stem, next.stem, by < 0 ? "before" : "after");
}

/** One deck moved in the list: dragged onto another, or nudged a place. */
export type DeckMove =
  | { kind: "drop"; from: string; to: string; side: "before" | "after" }
  | { kind: "nudge"; stem: string; by: -1 | 1 };

/**
 * The deck list's stems after `move`, made on `decks` as `order` lays them
 * out. Null for a move that moves nothing there.
 */
export function orderAfter(
  decks: readonly DeckEntry[],
  order: readonly string[],
  move: DeckMove,
): string[] | null {
  const shown = familyOrder(decks, order);
  return move.kind === "drop"
    ? moveDeck(shown, move.from, move.to, move.side)
    : nudgeDeck(shown, move.stem, move.by);
}

/**
 * `move` as an edit of `meldweb.toml`: made on the order the file has when
 * it is committed, not the one the page showed, so a move that meets a
 * change from another device is made again on that device's order rather
 * than writing over it (ADR-0028).
 */
export function moveInSettings(
  decks: readonly DeckEntry[],
  move: DeckMove,
): (settings: string | null) => string | null {
  return (settings) => {
    const read = readSettings(settings);
    if (read.kind === "refused") throw new Error(read.message);
    const next = orderAfter(decks, read.decks, move);
    return next && order_decks(settings ?? undefined, JSON.stringify(next));
  };
}

/** Each shown fork's parent, as `familyOrder` nests them. */
function parents(shown: readonly DeckEntry[]): Map<string, string> {
  const stems = new Set(shown.map((d) => d.stem));
  const out = new Map<string, string>();
  for (const d of shown) {
    if (d.parent && d.parent !== d.stem && stems.has(d.parent))
      out.set(d.stem, d.parent);
  }
  return out;
}

/** `stem` and the decks that are forks of it, or of those, in `shown`. */
function familyOf(shown: readonly DeckEntry[], stem: string): Set<string> {
  const family = new Set([stem]);
  // `shown` puts a fork after its parent, so one pass finds them all.
  for (const d of shown) {
    if (d.parent && d.stem !== stem && family.has(d.parent)) family.add(d.stem);
  }
  return family;
}
