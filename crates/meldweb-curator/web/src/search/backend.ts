import { type Copy, viaCopy } from "../copy";
import type { Found } from "../deck.gen";
import { imageUris } from "../scryfall";
import { cachedOne } from "../scryfallCache";
import { API, SEARCH_GATE, scryfallFetch } from "../scryfallQueue";

/** A card as the search overlay shows it. */
export interface ResultCard {
  /** Scryfall's id for the printing shown: stable, so a React key. */
  id: string;
  name: string;
  typeLine: string;
  manaCost: string;
  /** The front face's image, absent for the rare card with none. */
  image?: string;
  set: string;
  num: string;
}

/** One page of answers. */
export interface SearchPage {
  cards: ResultCard[];
  /** Every card the query matches, across all pages. */
  total: number;
  /**
   * What the backend ignored or changed in the query. Scryfall drops unknown
   * terms and answers anyway, so without these a typo silently widens a search.
   */
  warnings: string[];
  /** The next page, when there is one. */
  more?: () => Promise<SearchPage>;
}

/** A query the backend refused outright, with its reasons. */
export class SearchRefused extends Error {
  constructor(
    message: string,
    readonly warnings: string[],
  ) {
    super(message);
  }
}

/**
 * Where searches go: query text in, results out. The overlay knows nothing
 * else, so a local search over bulk data can stand in for the API later.
 */
export interface SearchBackend {
  search(query: string, signal?: AbortSignal): Promise<SearchPage>;
}

/**
 * The page's copy of Scryfall, or before there is one Scryfall's
 * `/cards/search` on the 2-a-second queue. A query the copy cannot read goes
 * to Scryfall too: the copy refuses what it does not know rather than
 * guessing, and Scryfall may know it.
 */
export const scryfallSearch: SearchBackend = {
  search: (query, signal) => {
    const api = () =>
      fetchPage(`${API}/cards/search?q=${encodeURIComponent(query)}`, signal);
    return viaCopy(
      (copy) => copyPage(copy, query, 0, api),
      api,
      (page) => page.total > 0,
    );
  },
};

/** Scryfall's page size, which the copy pages by too. */
const PAGE = 175;

async function copyPage(
  copy: Copy,
  query: string,
  offset: number,
  api: () => Promise<SearchPage>,
): Promise<SearchPage> {
  const answer = await copy.search(query, offset, PAGE);
  if (answer.kind === "refused") return api();
  const next = offset + PAGE;
  return {
    cards: answer.cards.map(fromCopy),
    total: answer.total,
    warnings: [],
    ...(next < answer.total
      ? { more: () => copyPage(copy, query, next, api) }
      : {}),
  };
}

function fromCopy(f: Found): ResultCard {
  return {
    id: f.id,
    name: f.name,
    typeLine: f.typeLine,
    manaCost: f.manaCost,
    ...(f.image ? { image: f.image } : {}),
    set: f.set,
    num: f.num,
  };
}

/** A page as the cache keeps it: plain data, with the next page by its URL. */
type KeptPage =
  | {
      kind: "page";
      cards: ResultCard[];
      total: number;
      warnings: string[];
      next?: string;
    }
  | { kind: "refused"; message: string; warnings: string[] };

/**
 * One page, cached by its URL. `signal` is the request's own: a search given
 * up on is never sent if it is still queued, and leaves nothing cached.
 */
async function fetchPage(
  url: string,
  signal?: AbortSignal,
): Promise<SearchPage> {
  const kept = await cachedOne<KeptPage>(
    ["scryfall", "search", url],
    async (own) => {
      const response = await scryfallFetch(SEARCH_GATE, url, {
        signal: signal ?? own,
      });
      const json: unknown = await response.json();
      try {
        const { cards, total, warnings } = readPage(response.status, json, () =>
          Promise.reject(new Error("not followed here")),
        );
        const body = (json ?? {}) as Record<string, unknown>;
        const next =
          body.has_more === true && typeof body.next_page === "string"
            ? body.next_page
            : undefined;
        return {
          kind: "page",
          cards,
          total,
          warnings,
          ...(next ? { next } : {}),
        };
      } catch (e) {
        if (!(e instanceof SearchRefused)) throw e;
        return { kind: "refused", message: e.message, warnings: e.warnings };
      }
    },
  );
  if (kept.kind === "refused")
    throw new SearchRefused(kept.message, kept.warnings);
  const { next, kind: _, ...page } = kept;
  return next ? { ...page, more: () => fetchPage(next, signal) } : page;
}

const strings = (x: unknown): string[] =>
  Array.isArray(x) ? x.filter((s): s is string => typeof s === "string") : [];

/**
 * Scryfall's answer as a page. A 404 is how it says "no cards", not an error;
 * a 400 is every term dropped, which is a refusal carrying its reasons.
 */
export function readPage(
  status: number,
  json: unknown,
  follow: (next: string) => Promise<SearchPage>,
): SearchPage {
  const body = (json ?? {}) as Record<string, unknown>;
  const warnings = strings(body.warnings);
  if (status === 404) return { cards: [], total: 0, warnings };
  if (status !== 200 || body.object !== "list") {
    const details =
      typeof body.details === "string"
        ? body.details
        : `Scryfall answered ${status}`;
    throw new SearchRefused(details, warnings);
  }
  const cards = (Array.isArray(body.data) ? body.data : []).flatMap(readCard);
  const next = body.has_more === true ? body.next_page : undefined;
  return {
    cards,
    total: typeof body.total_cards === "number" ? body.total_cards : 0,
    warnings,
    ...(typeof next === "string" ? { more: () => follow(next) } : {}),
  };
}

function readCard(raw: unknown): ResultCard[] {
  const c = raw as Record<string, unknown>;
  const uris = imageUris(c);
  const front = Array.isArray(c.card_faces)
    ? ((c.card_faces[0] ?? {}) as Record<string, unknown>)
    : {};
  if (
    typeof c.id !== "string" ||
    typeof c.name !== "string" ||
    typeof c.set !== "string" ||
    typeof c.collector_number !== "string"
  )
    return [];
  const text = (x: unknown) => (typeof x === "string" ? x : "");
  return [
    {
      id: c.id,
      name: c.name,
      typeLine: text(c.type_line ?? front.type_line),
      manaCost: text(c.mana_cost ?? front.mana_cost),
      ...(typeof uris?.normal === "string" ? { image: uris.normal } : {}),
      set: c.set,
      num: c.collector_number,
    },
  ];
}
