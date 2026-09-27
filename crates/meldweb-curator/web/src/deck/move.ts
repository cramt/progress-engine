/**
 * A card's categories after it is dropped on `to`, having been dragged out of
 * the group for `from` (null when it was in no category).
 *
 * A plain drop moves it: `to` takes `from`'s place in the list. A secondary
 * drop (Ctrl, as in Archidekt) keeps every category and adds `to` behind them.
 */
export function dropOnto(
  categories: readonly string[],
  from: string | null,
  to: string,
  secondary: boolean,
): string[] {
  if (categories.includes(to)) {
    return secondary || from === null || from === to
      ? [...categories]
      : categories.filter((c) => c !== from);
  }
  if (secondary || from === null) return [...categories, to];
  return categories.map((c) => (c === from ? to : c));
}
