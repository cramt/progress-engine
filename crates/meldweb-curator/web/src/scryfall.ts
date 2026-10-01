import type { CardRef } from "./deck";
import { cachedMany, scryfallClient } from "./scryfallCache";
import { API, SEARCH_GATE, scryfallFetch } from "./scryfallQueue";

/**
 * What the editor needs of a Scryfall card: a picture of it, and its colour
 * identity (as Scryfall's letters, `W U B R G`) for the search's smart filters.
 */
export interface Printing {
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

// `v2` drops what was cached before a printing carried its set and number.
const PRINTING = ["scryfall", "printing", "v2"] as const;
const IN_SET = ["scryfall", "in-set"] as const;

function toPrinting(card: CollectionCard): Printing {
  return {
    name: card.name,
    set: card.set,
    num: card.collector_number,
    image: card.image,
    colorIdentity: card.colorIdentity,
    typeLine: card.typeLine,
    ...(card.prints ? { prints: card.prints } : {}),
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

/**
 * Looks up every card's printing, from the cache where any lookup has found
 * it before and otherwise in as few requests as Scryfall allows. Cards
 * Scryfall cannot find are absent from the map rather than an error: the deck
 * is still the deck, and the view shows what the file names instead. A batch
 * still queued when `signal` aborts is never sent.
 */
export async function fetchPrintings(
  cards: readonly { card: CardRef }[],
  signal?: AbortSignal,
): Promise<Printings> {
  return cachedMany(
    PRINTING,
    cards.map((c) => c.card),
    printingKey,
    async (misses) =>
      remember(await collection(misses.map(identifier), signal)),
  );
}

interface CollectionCard {
  name: string;
  set: string;
  collector_number: string;
  image: string;
  colorIdentity: string[];
  typeLine: string;
  prints?: string;
}

function frontTypeLine(c: Record<string, unknown>): string {
  const faces = Array.isArray(c.card_faces)
    ? (c.card_faces as Record<string, unknown>[])
    : [];
  const line = c.type_line ?? faces[0]?.type_line;
  return typeof line === "string" ? (line.split(" // ")[0] ?? line) : "";
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
      typeof c.name !== "string" ||
      typeof c.set !== "string" ||
      typeof c.collector_number !== "string" ||
      typeof image !== "string"
    ) {
      return [];
    }
    return [
      {
        name: c.name,
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
  const found = await cachedMany(IN_SET, wanted, key, async (misses) => {
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
  return wanted.map((w) => found.get(key(w)) ?? null);
}
