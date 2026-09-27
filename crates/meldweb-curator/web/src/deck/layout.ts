import type { Card, Category, Kind } from "../deck";

export interface Group {
  /** The category's name, or null for cards that are in none. */
  category: string | null;
  kind?: Kind;
  cards: Card[];
  qty: number;
}

export const UNCATEGORIZED = "Uncategorized";

/** Commander first, then the deck, then everything outside it. */
function rank(kind: Kind | undefined): number {
  switch (kind) {
    case "commander":
      return 0;
    case undefined:
    case "in-deck":
      return 1;
    default:
      return 2;
  }
}

/**
 * One group per category that holds a card, and a card appears in every group
 * it is in: a category is a question about the deck ("what bounces?"), and the
 * answer should not depend on which category the card was filed under first.
 */
export function groupByCategory(
  categories: readonly Category[],
  cards: readonly Card[],
  nameOf: (card: Card) => string,
): Group[] {
  const kinds = new Map(categories.map((c) => [c.name, c.kind]));
  const members = new Map<string | null, Card[]>();
  for (const card of cards) {
    for (const name of card.categories.length > 0 ? card.categories : [null]) {
      members.set(name, [...(members.get(name) ?? []), card]);
    }
  }
  const groups = [...members].map(([category, members]): Group => {
    const kind = category === null ? undefined : kinds.get(category);
    return {
      category,
      ...(kind === undefined ? {} : { kind }),
      cards: members.toSorted(
        (a, b) => nameOf(a).localeCompare(nameOf(b)) || a.index - b.index,
      ),
      qty: members.reduce((n, c) => n + c.qty, 0),
    };
  });
  const label = (g: Group) => g.category ?? UNCATEGORIZED;
  return groups.toSorted(
    (a, b) => rank(a.kind) - rank(b.kind) || label(a).localeCompare(label(b)),
  );
}

/**
 * Masonry the way Archidekt packs it: each group goes into the column that is
 * currently shortest, so the first row reads left to right in order. Heights
 * are known without measuring, because a stack's height follows from its size.
 */
export function packColumns<T>(
  items: readonly T[],
  height: (item: T) => number,
  columns: number,
): T[][] {
  const packed: T[][] = Array.from({ length: Math.max(1, columns) }, () => []);
  const heights = packed.map(() => 0);
  for (const item of items) {
    const shortest = heights.indexOf(Math.min(...heights));
    packed[shortest]?.push(item);
    heights[shortest] = (heights[shortest] ?? 0) + height(item);
  }
  return packed;
}
