import type { Finish } from "../deck";
import { SEARCH_GATE, scryfallFetch } from "../scryfallQueue";

/** One printing of a card, as the printing dropdown and the grid show it. */
export interface PrintingOption {
  name: string;
  set: string;
  num: string;
  setName: string;
  /** `YYYY-MM-DD`. */
  released: string;
  image?: string;
  small?: string;
  finishes: Finish[];
}

/** `set/num`, lowercased: how a printing is compared with the deck's. */
export function printingId(p: { set: string; num: string }): string {
  return `${p.set.toLowerCase()}/${p.num.toLowerCase()}`;
}

const FINISHES: readonly string[] = ["nonfoil", "foil", "etched"];

/** One page of Scryfall's search, checked into `PrintingOption`s. */
export function parsePrintsPage(json: unknown): {
  printings: PrintingOption[];
  next: string | null;
} {
  const page = json as {
    data?: unknown;
    has_more?: unknown;
    next_page?: unknown;
  };
  if (!Array.isArray(page.data))
    throw new Error("Scryfall's list of printings has no data");
  const printings = page.data.flatMap((raw: unknown): PrintingOption[] => {
    const c = raw as Record<string, unknown>;
    const faces = Array.isArray(c.card_faces)
      ? (c.card_faces as Record<string, unknown>[])
      : [];
    const uris = (c.image_uris ?? faces[0]?.image_uris) as
      | Record<string, unknown>
      | undefined;
    if (
      typeof c.name !== "string" ||
      typeof c.set !== "string" ||
      typeof c.collector_number !== "string"
    )
      return [];
    const finishes = Array.isArray(c.finishes)
      ? (c.finishes.filter(
          (f): f is Finish => typeof f === "string" && FINISHES.includes(f),
        ) as Finish[])
      : [];
    return [
      {
        name: c.name,
        set: c.set.toLowerCase(),
        num: c.collector_number,
        setName: typeof c.set_name === "string" ? c.set_name : c.set,
        released: typeof c.released_at === "string" ? c.released_at : "",
        ...(typeof uris?.normal === "string" ? { image: uris.normal } : {}),
        ...(typeof uris?.small === "string" ? { small: uris.small } : {}),
        finishes,
      },
    ];
  });
  const next =
    page.has_more === true && typeof page.next_page === "string"
      ? page.next_page
      : null;
  return { printings, next };
}

/** Newest first, the way Scryfall and Archidekt list them; `oldest` reverses it. */
export function byRelease(
  printings: readonly PrintingOption[],
  oldest = false,
): PrintingOption[] {
  const sorted = printings.toSorted(
    (a, b) =>
      b.released.localeCompare(a.released) ||
      a.set.localeCompare(b.set) ||
      a.num.localeCompare(b.num, undefined, { numeric: true }),
  );
  return oldest ? sorted.reverse() : sorted;
}

/** The printings whose set name or code contains `filter`, ignoring case. */
export function filterBySet(
  printings: readonly PrintingOption[],
  filter: string,
): PrintingOption[] {
  const f = filter.trim().toLowerCase();
  if (!f) return [...printings];
  return printings.filter(
    (p) => p.setName.toLowerCase().includes(f) || p.set.includes(f),
  );
}

/**
 * Scryfall's search for every printing of a card, for when the deck has no
 * `prints_search_uri` for it. The URI Scryfall gives is better: it searches
 * by oracle id, which also finds reversible printings a name search misses.
 */
export function printsByName(name: string): string {
  const q = encodeURIComponent(`!"${name}"`);
  return `https://api.scryfall.com/cards/search?q=${q}&unique=prints&order=released&include_extras=true`;
}

/** Scryfall allows `/cards/search` 2 requests a second: the shared search queue's pace. */
export const SEARCH_INTERVAL_MS = SEARCH_GATE.intervalMs;

/**
 * A search request on the shared search queue, which spaces it from every
 * other search, named or collection request and holds off 30 s after a 429.
 * A request whose signal was aborted while it waited is never sent, so
 * stepping quickly through a deck costs nothing for the cards stepped past.
 */
async function searchRequest(
  url: string,
  signal?: AbortSignal,
): Promise<unknown> {
  const response = await scryfallFetch(
    SEARCH_GATE,
    url,
    signal ? { signal } : {},
  );
  // No printing matched is a 404 with an error object, not an empty list.
  if (response.status === 404) return { data: [] };
  if (!response.ok) throw new Error(`Scryfall answered ${response.status}`);
  return response.json();
}

const cache = new Map<string, Promise<PrintingOption[]>>();

/** Every printing behind a `prints_search_uri`, all pages, asked for once. */
export function fetchAllPrintings(
  uri: string,
  signal?: AbortSignal,
): Promise<PrintingOption[]> {
  const cached = cache.get(uri);
  if (cached) return cached;
  const load = async () => {
    const all: PrintingOption[] = [];
    let url: string | null = uri;
    while (url) {
      const page = parsePrintsPage(await searchRequest(url, signal));
      all.push(...page.printings);
      url = page.next;
    }
    return all;
  };
  const promise = load();
  cache.set(uri, promise);
  // A failed or abandoned lookup is asked again next time.
  promise.catch(() => cache.delete(uri));
  return promise;
}
