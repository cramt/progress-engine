import type { OwnedCard } from "../collection";
import type { Finish } from "../deck";

/** A column of the collection's table that its lines can be ordered by. */
export type SortField = "qty" | "name" | "printing" | "finish" | "price";

export interface Sort {
  by: SortField;
  descending: boolean;
}

export const BY_NAME: Sort = { by: "name", descending: false };

const FIELDS: readonly SortField[] = [
  "qty",
  "name",
  "printing",
  "finish",
  "price",
];

const FINISH_ORDER: readonly Finish[] = ["nonfoil", "foil", "etched"];

/**
 * The sort a click on `field`'s header asks for: the other way round when it
 * is already the sort, or else a fresh one, biggest first for the numbers,
 * since the most copies and the dearest lines are what a number is sorted for.
 */
export function clickSort(current: Sort, field: SortField): Sort {
  if (current.by === field)
    return { by: field, descending: !current.descending };
  return { by: field, descending: field === "qty" || field === "price" };
}

/**
 * How two lines compare under `sort`. Ties, whichever way round, fall to the
 * name and then the file's order, so equal lines never swap between renders.
 * A line with no printing or no price goes last either way: it has no value
 * to be big or small by.
 */
export function ownedOrder(
  sort: Sort,
  nameOf: (card: OwnedCard) => string,
  priceOf: (card: OwnedCard) => number | undefined,
): (a: OwnedCard, b: OwnedCard) => number {
  const byName = (a: OwnedCard, b: OwnedCard) =>
    nameOf(a).localeCompare(nameOf(b)) || a.index - b.index;
  const sign = sort.descending ? -1 : 1;
  const missingLast = <T>(
    x: T | undefined,
    y: T | undefined,
    cmp: (x: T, y: T) => number,
  ): number => {
    if (x === undefined || y === undefined)
      return (x === undefined ? 1 : 0) - (y === undefined ? 1 : 0);
    return sign * cmp(x, y);
  };
  const printing = (c: OwnedCard) =>
    c.card.kind === "printing" ? c.card : undefined;
  const field = (a: OwnedCard, b: OwnedCard): number => {
    switch (sort.by) {
      case "qty":
        return sign * (a.qty - b.qty);
      case "name":
        return sign * nameOf(a).localeCompare(nameOf(b));
      case "printing":
        return missingLast(
          printing(a),
          printing(b),
          (x, y) =>
            x.set.localeCompare(y.set) ||
            x.num.localeCompare(y.num, undefined, { numeric: true }),
        );
      case "finish":
        return (
          sign *
          (FINISH_ORDER.indexOf(a.finish) - FINISH_ORDER.indexOf(b.finish))
        );
      case "price":
        return missingLast(priceOf(a), priceOf(b), (x, y) => x - y);
    }
  };
  return (a, b) => field(a, b) || byName(a, b);
}

/** Places folded shut, by name; unsorted is `null`. */
export type Folded = ReadonlySet<string | null>;

export interface CollectionView {
  sort: Sort;
  folded: Folded;
}

const KEY = "meldweb.collection-view";

/**
 * The sort and folds last left in this browser, each checked, so a stale or
 * hand-edited entry costs only itself. A fold on a place since renamed or
 * removed is harmless and simply never matches.
 */
export function loadView(
  storage: Pick<Storage, "getItem"> | undefined = globalThis.localStorage,
): CollectionView {
  let raw: Record<string, unknown> = {};
  try {
    const parsed: unknown = JSON.parse(storage?.getItem(KEY) ?? "{}");
    if (typeof parsed === "object" && parsed !== null)
      raw = parsed as Record<string, unknown>;
  } catch {
    // unreadable or blocked storage: the defaults
  }
  const sort = raw.sort as Record<string, unknown> | null | undefined;
  const by = FIELDS.find((f) => f === sort?.by);
  return {
    sort:
      by && typeof sort?.descending === "boolean"
        ? { by, descending: sort.descending }
        : BY_NAME,
    folded: new Set(
      Array.isArray(raw.folded)
        ? raw.folded.filter(
            (f): f is string | null => f === null || typeof f === "string",
          )
        : [],
    ),
  };
}

export function saveView(
  view: CollectionView,
  storage: Pick<Storage, "setItem"> | undefined = globalThis.localStorage,
): void {
  try {
    storage?.setItem(
      KEY,
      JSON.stringify({ sort: view.sort, folded: [...view.folded] }),
    );
  } catch {
    // a private window, or storage blocked: the view lasts this session
  }
}
