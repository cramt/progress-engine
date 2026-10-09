import { type Copy, viaCopyEach } from "./copy";
import type { CardRef, Finish } from "./deck";
import type { Ask, Found, ScryfallCard } from "./deck.gen";
import { cachedMany, scryfallClient } from "./scryfallCache";
import { API, SEARCH_GATE, scryfallFetch } from "./scryfallQueue";

/**
 * What the editor needs of a Scryfall card: a picture of it, and its colour
 * identity (as Scryfall's letters, `W U B R G`) for the search's smart filters.
 */
export interface Printing {
  /** Scryfall's id for the printing, which Archidekt calls a card's `uid`. */
  id: string;
  name: string;
  /** Which printing this is, which for a card named by name is Scryfall's pick. */
  set: string;
  num: string;
  image: string;
  colorIdentity: readonly string[];
  /** The front face's type line, `Battle — Siege` for an Invasion. */
  typeLine: string;
  /** Scryfall's search for every printing of the card, when it gave one. */
  prints?: string;
  /** How the front is turned to be read, when not upright. */
  turn?: Turn;
  /** The other face, for a double-faced card or a flip card. */
  back?: Face;
}

/**
 * Which way a card's picture is turned to be read: a battle's front and a
 * split card sideways, a flip card's other half upside down.
 */
export type Turn = "upright" | "sideways" | "upside-down";

export interface Face {
  image: string;
  turn: Turn;
}

/**
 * How a Scryfall card's faces read: the front's turn, and the back where it
 * has one. A battle is a `transform` card whose front alone is a Battle, so
 * it is told by the type line, not the layout. Aftermath is a split card whose
 * top half reads upright, so it is not turned; a meld card's back is another
 * card, so it has none here.
 */
export function faces(card: Record<string, unknown>): {
  turn?: Turn;
  back?: Face;
} {
  const cardFaces = Array.isArray(card.card_faces)
    ? (card.card_faces as Record<string, unknown>[])
    : [];
  const keywords = Array.isArray(card.keywords) ? card.keywords : [];
  const typeLine = cardFaces[0]?.type_line ?? card.type_line;
  const sideways =
    (typeof typeLine === "string" && typeLine.startsWith("Battle")) ||
    (card.layout === "split" && !keywords.includes("Aftermath"));
  const turn: { turn?: Turn } = sideways ? { turn: "sideways" } : {};
  const front = imageUris(card)?.normal;
  if (card.layout === "flip" && typeof front === "string")
    return { ...turn, back: { image: front, turn: "upside-down" } };
  const back = (cardFaces[1]?.image_uris as Record<string, unknown> | undefined)
    ?.normal;
  return typeof back === "string"
    ? { ...turn, back: { image: back, turn: "upright" } }
    : turn;
}

/** A card's printing, keyed the way the deck names it. */
export type Printings = ReadonlyMap<string, Printing>;

/** `set/num`, lowercased: how a printing is compared with the deck's. */
export function printingId(p: { set: string; num: string }): string {
  return `${p.set.toLowerCase()}/${p.num.toLowerCase()}`;
}

export function printingKey(ref: CardRef): string {
  return ref.kind === "printing"
    ? printingId(ref)
    : `name:${ref.name.toLowerCase()}`;
}

/**
 * A card's name: the file's, or Scryfall's for a card named by printing, or
 * `set/num` for a printing Scryfall has not named.
 */
export function cardName(
  card: { card: CardRef },
  printings: Printings,
): string {
  const ref = card.card;
  return ref.kind === "name"
    ? ref.name
    : (printings.get(printingKey(ref))?.name ?? `${ref.set}/${ref.num}`);
}

/**
 * A Scryfall card's `image_uris`: its own, or its front face's for a
 * double-faced card, which has them per face.
 */
export function imageUris(
  card: Record<string, unknown>,
): Record<string, unknown> | undefined {
  const faces = Array.isArray(card.card_faces)
    ? (card.card_faces as Record<string, unknown>[])
    : [];
  return (card.image_uris ?? faces[0]?.image_uris) as
    | Record<string, unknown>
    | undefined;
}

/**
 * The art alone of a card whose picture is Scryfall's `normal` image: the
 * same file under `art_crop`. Any other URL comes back as it is.
 */
export function artCrop(image: string): string {
  return image.replace("/normal/", "/art_crop/");
}

type Identifier = { set: string; collector_number: string } | { name: string };

function identifier(ref: CardRef): Identifier {
  return ref.kind === "printing"
    ? { set: ref.set, collector_number: ref.num }
    : { name: frontFace(ref.name) };
}

// Scryfall finds a double-faced card by its front face alone: named
// `A // B` in whole, `/cards/collection` answers not found.
function frontFace(name: string): string {
  return name.split(" // ")[0] ?? name;
}

// Scryfall's limit per request.
const BATCH = 75;

// `v4` drops what was cached before a printing carried its faces, and what
// was cached while a battle was looked for by layout, which left it upright.
const PRINTING = ["scryfall", "printing", "v4"] as const;
const IN_SET = ["scryfall", "in-set"] as const;
// Unlike the rest, a price moves: Scryfall refreshes them once a day, so each
// day's are kept under that day and the days before dropped.
const PRICE = ["scryfall", "price"] as const;

/** What one copy sells for, by currency and finish, where Scryfall knows. */
export type Currency = "eur" | "usd";
export type CardPrices = Readonly<
  Record<Currency, Readonly<Partial<Record<Finish, number>>>>
>;

/** Today's price day, in UTC as Scryfall's daily refresh is. */
function priceDay(): string {
  return new Date().toISOString().slice(0, 10);
}

function rememberPrices(cards: readonly CollectionCard[]): void {
  const day = priceDay();
  for (const card of cards)
    for (const key of printingKeys(card))
      scryfallClient.setQueryData([...PRICE, day, key], card.prices);
}

function toPrinting(card: CollectionCard): Printing {
  return {
    id: card.id,
    name: card.name,
    set: card.set,
    num: card.collector_number,
    image: card.image,
    colorIdentity: card.colorIdentity,
    typeLine: card.typeLine,
    ...(card.prints ? { prints: card.prints } : {}),
    ...(card.turn ? { turn: card.turn } : {}),
    ...(card.back ? { back: card.back } : {}),
  };
}

/**
 * The keys a found card answers to: its printing, and, since a name lookup
 * returns whichever printing Scryfall prefers, its name, front face included
 * for double-faced cards.
 */
function printingKeys(card: CollectionCard): string[] {
  const keys = [
    printingId({ set: card.set, num: card.collector_number }),
    `name:${card.name.toLowerCase()}`,
  ];
  const front = card.name.split(" // ")[0];
  if (front !== undefined) keys.push(`name:${front.toLowerCase()}`);
  return keys;
}

/** `/cards/collection` for `identifiers`, 75 to a request. */
async function collection(
  identifiers: readonly object[],
  signal?: AbortSignal,
): Promise<CollectionCard[]> {
  const cards: CollectionCard[] = [];
  for (let i = 0; i < identifiers.length; i += BATCH) {
    const response = await scryfallFetch(
      SEARCH_GATE,
      `${API}/cards/collection`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ identifiers: identifiers.slice(i, i + BATCH) }),
        ...(signal ? { signal } : {}),
      },
    );
    if (!response.ok) throw new Error(`Scryfall answered ${response.status}`);
    cards.push(...parseCollection(await response.json()));
  }
  // Every answer carries today's prices, so a lookup for anything else is
  // one for prices too.
  rememberPrices(cards);
  return cards;
}

/** Remembers each card under every key it answers to. */
function remember(cards: readonly CollectionCard[]): Map<string, Printing> {
  const found = new Map<string, Printing>();
  for (const card of cards) {
    const printing = toPrinting(card);
    for (const key of printingKeys(card)) found.set(key, printing);
  }
  return found;
}

/** A printing the copy found, as the editor shows it. */
function fromCopy(f: Found): Printing {
  return {
    id: f.id,
    name: f.name,
    set: f.set,
    num: f.num,
    // A printing with no picture is dropped by `fromCopyAll`, as
    // `parseCollection` drops one off the API.
    image: f.image ?? "",
    colorIdentity: f.colorIdentity,
    typeLine: f.frontTypeLine,
    ...(f.prints ? { prints: f.prints } : {}),
    ...(f.turn ? { turn: f.turn } : {}),
    ...(f.back ? { back: f.back } : {}),
  };
}

/**
 * Each distinct card of `refs` looked up in the copy, keyed as
 * `fetchPrintings` keys them, with what `pick` makes of it; a card the copy
 * has no picture of is absent.
 */
async function fromCopyAll<T>(
  copy: Copy,
  refs: readonly CardRef[],
  pick: (found: Found) => T,
): Promise<Map<string, T>> {
  const wanted = [...new Map(refs.map((r) => [printingKey(r), r])).entries()];
  const found = await copy.lookup(wanted.map(([, r]) => r));
  const out = new Map<string, T>();
  wanted.forEach(([key], i) => {
    const f = found[i];
    if (f?.image) out.set(key, pick(f));
  });
  return out;
}

/**
 * Looks up every card's printing: in the page's copy of Scryfall, or before
 * there is one from the cache where any lookup has found it before and
 * otherwise in as few requests as Scryfall allows. Cards Scryfall cannot find
 * are absent from the map rather than an error: the deck is still the deck,
 * and the view shows what the file names instead. A batch still queued when
 * `signal` aborts is never sent.
 */
export async function fetchPrintings(
  cards: readonly { card: CardRef }[],
  signal?: AbortSignal,
): Promise<Printings> {
  const refs = cards.map((c) => c.card);
  return viaCopyEach(
    refs,
    printingKey,
    (copy, refs) => fromCopyAll(copy, refs, fromCopy),
    (refs) =>
      cachedMany(PRINTING, refs, printingKey, async (misses) =>
        remember(await collection(misses.map(identifier), signal)),
      ),
  );
}

/**
 * Today's price of every card, keyed as `fetchPrintings` keys them. A card
 * named by name is priced as the printing Scryfall picks for it. Cards
 * Scryfall cannot find are absent.
 */
export async function fetchPrices(
  cards: readonly { card: CardRef }[],
  signal?: AbortSignal,
): Promise<ReadonlyMap<string, CardPrices>> {
  const day = priceDay();
  scryfallClient.removeQueries({
    queryKey: PRICE,
    predicate: (q) => q.queryKey[2] !== day,
  });
  const refs = cards.map((c) => c.card);
  return viaCopyEach(
    refs,
    printingKey,
    (copy, refs) =>
      fromCopyAll(copy, refs, (f) => ({
        eur: f.prices.eur ?? {},
        usd: f.prices.usd ?? {},
      })),
    (refs) =>
      cachedMany([...PRICE, day], refs, printingKey, async (misses) => {
        const found = new Map<string, CardPrices>();
        for (const card of await collection(misses.map(identifier), signal))
          for (const key of printingKeys(card)) found.set(key, card.prices);
        return found;
      }),
  );
}

interface CollectionCard {
  id: string;
  name: string;
  prices: CardPrices;
  set: string;
  collector_number: string;
  image: string;
  colorIdentity: string[];
  typeLine: string;
  prints?: string;
  turn?: Turn;
  back?: Face;
}

function frontTypeLine(c: Record<string, unknown>): string {
  const faces = Array.isArray(c.card_faces)
    ? (c.card_faces as Record<string, unknown>[])
    : [];
  const line = c.type_line ?? faces[0]?.type_line;
  return typeof line === "string" ? (line.split(" // ")[0] ?? line) : "";
}

/** Scryfall's prices are decimal strings, or null where it has none. */
function parsePrices(raw: unknown): CardPrices {
  const p = (raw ?? {}) as Record<string, unknown>;
  const byFinish = (
    fields: Record<Finish, unknown>,
  ): Partial<Record<Finish, number>> =>
    Object.fromEntries(
      Object.entries(fields).flatMap(([finish, v]) => {
        const n = typeof v === "string" ? Number(v) : Number.NaN;
        return Number.isFinite(n) ? [[finish, n]] : [];
      }),
    );
  return {
    eur: byFinish({ nonfoil: p.eur, foil: p.eur_foil, etched: p.eur_etched }),
    usd: byFinish({ nonfoil: p.usd, foil: p.usd_foil, etched: p.usd_etched }),
  };
}

/** Scryfall's JSON is outside our types until it has been checked. */
function parseCollection(json: unknown): CollectionCard[] {
  const data = (json as { data?: unknown }).data;
  if (!Array.isArray(data))
    throw new Error("Scryfall collection response has no data");
  return data.flatMap((raw: unknown): CollectionCard[] => {
    const c = raw as Record<string, unknown>;
    const image = imageUris(c)?.normal;
    if (
      typeof c.id !== "string" ||
      typeof c.name !== "string" ||
      typeof c.set !== "string" ||
      typeof c.collector_number !== "string" ||
      typeof image !== "string"
    ) {
      return [];
    }
    return [
      {
        id: c.id,
        name: c.name,
        prices: parsePrices(c.prices),
        set: c.set.toLowerCase(),
        // Scryfall matches collector numbers case-sensitively (The List's `RIX-1`).
        collector_number: c.collector_number,
        image,
        colorIdentity: Array.isArray(c.color_identity)
          ? c.color_identity.filter((x): x is string => typeof x === "string")
          : [],
        typeLine: frontTypeLine(c),
        ...(typeof c.prints_search_uri === "string"
          ? { prints: c.prints_search_uri }
          : {}),
        ...faces(c),
      },
    ];
  });
}

/**
 * Each card's printing in the set given with it, `null` where Scryfall has no
 * card of that name there. A set holds several printings of some cards, and
 * Scryfall picks one the way Archidekt does for a line with no number. What
 * it finds is also cached as a printing, so the deck it lands in opens with
 * its pictures already known.
 */
export async function fetchPrintingsInSets(
  wanted: readonly { name: string; set: string }[],
  signal?: AbortSignal,
): Promise<({ set: string; num: string } | null)[]> {
  const key = (w: { name: string; set: string }) =>
    `${w.set.toLowerCase()}:${frontFace(w.name).toLowerCase()}`;
  const fromApi = (wanted: readonly { name: string; set: string }[]) =>
    cachedMany(IN_SET, wanted, key, async (misses) => {
      const cards = await collection(
        misses.map((w) => ({ name: frontFace(w.name), set: w.set })),
        signal,
      );
      for (const [k, printing] of remember(cards))
        scryfallClient.setQueryData([...PRINTING, k], printing);
      return new Map(
        cards.map((card) => [
          key(card),
          { set: card.set, num: card.collector_number },
        ]),
      );
    });
  const found = await viaCopyEach(
    wanted,
    key,
    async (copy, wanted) => {
      const found = await copy.lookup(
        wanted.map((w) => ({ kind: "inSet", name: w.name, set: w.set })),
      );
      return new Map(
        wanted.flatMap((w, i) => {
          const f = found[i];
          return f ? [[key(w), { set: f.set, num: f.num }] as const] : [];
        }),
      );
    },
    fromApi,
  );
  return wanted.map((w) => found.get(key(w)) ?? null);
}

/**
 * Scryfall's card for each of an import's asks it knows, by id or by set and
 * collector number, in as few requests as it allows. What it finds is cached
 * as a printing too, so the collection opens on it with pictures known. An
 * ask it has no card for is absent; which row that leaves by name is Rust's
 * to say.
 */
export async function fetchAsked(
  asks: readonly Ask[],
  signal?: AbortSignal,
): Promise<ScryfallCard[]> {
  const fromApi = async (asks: readonly Ask[]) => {
    const cards = await collection(
      asks.map((a) =>
        a.kind === "id"
          ? { id: a.id }
          : { set: a.set, collector_number: a.num },
      ),
      signal,
    );
    for (const [k, printing] of remember(cards))
      scryfallClient.setQueryData([...PRINTING, k], printing);
    // Scryfall answers cards, not asks, so each is keyed both ways an ask
    // could have named it.
    return new Map(
      cards.flatMap((c) => {
        const card = {
          id: c.id,
          set: c.set,
          num: c.collector_number,
          name: c.name,
        };
        return [
          [askKey({ kind: "id", id: c.id }), card],
          [
            askKey({ kind: "printing", set: c.set, num: c.collector_number }),
            card,
          ],
        ] as const;
      }),
    );
  };
  const found = await viaCopyEach(
    asks,
    askKey,
    async (copy, asks) => {
      const found = await copy.lookup(asks);
      return new Map(
        asks.flatMap((a, i) => {
          const f = found[i];
          return f
            ? [
                [
                  askKey(a),
                  { id: f.id, set: f.set, num: f.num, name: f.name },
                ] as const,
              ]
            : [];
        }),
      );
    },
    fromApi,
  );
  return [...new Map([...found.values()].map((c) => [c.id, c])).values()];
}

function askKey(ask: Ask): string {
  return ask.kind === "id"
    ? `id:${ask.id}`
    : `${ask.set.toLowerCase()}/${ask.num.toLowerCase()}`;
}
