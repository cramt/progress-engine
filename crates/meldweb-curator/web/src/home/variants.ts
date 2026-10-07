import type { DeckEntry } from "../github/decks";

/**
 * Decks in the order `meldweb.toml` declares, by path, and those it does not
 * name after them by name; each deck's variants straight after it, so a
 * budget build sits beside the deck it came from. A variant whose parent is
 * gone, or is itself a variant, stands on its own.
 */
export function familyOrder(
  decks: readonly DeckEntry[],
  order: readonly string[] = [],
): DeckEntry[] {
  const rank = new Map(order.map((path, i) => [path, i]));
  const ordered = [...decks].sort(
    (a, b) =>
      (rank.get(a.path) ?? order.length) - (rank.get(b.path) ?? order.length) ||
      a.name.localeCompare(b.name),
  );
  const paths = new Set(decks.map((d) => d.path));
  const childrenOf = new Map<string, DeckEntry[]>();
  for (const d of ordered) {
    if (d.variantOf && d.variantOf !== d.path && paths.has(d.variantOf)) {
      childrenOf.set(d.variantOf, [...(childrenOf.get(d.variantOf) ?? []), d]);
    }
  }
  const placed = new Set<string>();
  const out: DeckEntry[] = [];
  const place = (d: DeckEntry) => {
    // A cycle of variants is a hand-edited file; each deck is placed once.
    if (placed.has(d.path)) return;
    placed.add(d.path);
    out.push(d);
    for (const child of childrenOf.get(d.path) ?? []) place(child);
  };
  for (const d of ordered) {
    const parented = d.variantOf && childrenOf.get(d.variantOf)?.includes(d);
    if (!parented) place(d);
  }
  // What a cycle left unplaced, in order.
  for (const d of ordered) place(d);
  return out;
}

/**
 * The deck list's paths after dragging `from` onto `to`, before or after it,
 * from `shown`, the list as `familyOrder` laid it out. A deck takes its
 * variants with it and moves among its siblings: dropped on a deck of another
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
  const rest = shown.filter((d) => !block.has(d.path));
  const family = familyOf(rest, target);
  let at = rest.findIndex((d) => d.path === target);
  if (at < 0) return null;
  if (side === "after") {
    while (family.has(rest[at + 1]?.path ?? "")) at++;
    at++;
  }
  const out = [
    ...rest.slice(0, at),
    ...shown.filter((d) => block.has(d.path)),
    ...rest.slice(at),
  ].map((d) => d.path);
  return out.every((p, i) => p === shown[i]?.path) ? null : out;
}

/**
 * The deck list's paths with `path` one place earlier (`-1`) or later (`1`)
 * among its siblings, as Alt+arrow and the tile's menu move it. Null at an end.
 */
export function nudgeDeck(
  shown: readonly DeckEntry[],
  path: string,
  by: -1 | 1,
): string[] | null {
  const parentOf = parents(shown);
  const level = parentOf.get(path);
  const siblings = shown.filter((d) => parentOf.get(d.path) === level);
  const at = siblings.findIndex((d) => d.path === path);
  const next = siblings[at + by];
  if (at < 0 || !next) return null;
  return moveDeck(shown, path, next.path, by < 0 ? "before" : "after");
}

/** Each shown variant's parent, as `familyOrder` nests them. */
function parents(shown: readonly DeckEntry[]): Map<string, string> {
  const paths = new Set(shown.map((d) => d.path));
  const out = new Map<string, string>();
  for (const d of shown) {
    if (d.variantOf && d.variantOf !== d.path && paths.has(d.variantOf))
      out.set(d.path, d.variantOf);
  }
  return out;
}

/** `path` and the decks that are variants of it, or of those, in `shown`. */
function familyOf(shown: readonly DeckEntry[], path: string): Set<string> {
  const family = new Set([path]);
  // `shown` puts a variant after its parent, so one pass finds them all.
  for (const d of shown) {
    if (d.variantOf && d.path !== path && family.has(d.variantOf))
      family.add(d.path);
  }
  return family;
}
