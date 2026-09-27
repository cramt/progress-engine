import type { Entry } from "../decklist";

export interface Group {
  name: string;
  commander: boolean;
  entries: Entry[];
  qty: number;
}

/** The category a card is shown under: its `{top}` one, else its first. */
export function premier(entry: Entry): string {
  const top = entry.categories.find((c) => c.flags.includes("top"));
  return (top ?? entry.categories[0])?.name ?? "Uncategorized";
}

/** Archidekt's order: the commander's category first, then alphabetical. */
export function groupByCategory(entries: readonly Entry[]): Group[] {
  const groups = [...Map.groupBy(entries, premier)].map(([name, members]) => ({
    name,
    commander: members.some((e) => e.commander),
    entries: members.toSorted((a, b) => a.name.localeCompare(b.name)),
    qty: members.reduce((n, e) => n + e.qty, 0),
  }));
  return groups.toSorted(
    (a, b) =>
      Number(b.commander) - Number(a.commander) || a.name.localeCompare(b.name),
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
