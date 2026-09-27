import type { Category } from "../decklist";

/**
 * A card's categories after it is dropped on `to`, having been dragged out of
 * `from`, the category it was shown under.
 *
 * A plain drop moves it: `to` takes `from`'s place and flags, so a `{top}`
 * premier stays premier. A secondary drop (Ctrl, as in Archidekt) keeps it
 * where it is and adds `to` behind the others.
 */
export function dropOnto(
  categories: readonly Category[],
  from: string,
  to: string,
  secondary: boolean,
): Category[] {
  const has = (name: string) => categories.some((c) => c.name === name);
  if (secondary) {
    return has(to) ? [...categories] : [...categories, { name: to, flags: [] }];
  }
  if (from === to) return [...categories];
  if (!has(from)) {
    // Shown as Uncategorized: it had nothing to move out of.
    return [
      { name: to, flags: [] },
      ...categories.filter((c) => c.name !== to),
    ];
  }
  return categories.flatMap((c) =>
    c.name === from ? [{ name: to, flags: c.flags }] : c.name === to ? [] : [c],
  );
}
