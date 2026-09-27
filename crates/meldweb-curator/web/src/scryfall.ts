import type { Card, CardRef } from "./deck";
import { API, SEARCH_GATE, scryfallFetch } from "./scryfallQueue";

/**
 * What the editor needs of a Scryfall card: a picture of it, and its colour
 * identity (as Scryfall's letters, `W U B R G`) for the search's smart filters.
 */
export interface Printing {
  name: string;
  image: string;
  colorIdentity: readonly string[];
  /** Scryfall's search for every printing of the card, when it gave one. */
  prints?: string;
}

/** A card's printing, keyed the way the deck names it. */
export type Printings = ReadonlyMap<string, Printing>;

export function printingKey(ref: CardRef): string {
  return ref.kind === "printing"
    ? `${ref.set.toLowerCase()}/${ref.num.toLowerCase()}`
    : `name:${ref.name.toLowerCase()}`;
}

type Identifier = { set: string; collector_number: string } | { name: string };

function identifier(ref: CardRef): Identifier {
  return ref.kind === "printing"
    ? { set: ref.set, collector_number: ref.num }
    : { name: ref.name };
}

// Scryfall's limit per request.
const BATCH = 75;

/**
 * Scryfall allows `/cards/collection` 2 requests a second, so a batch starts
 * no sooner than this after the one before it, through the search queue.
 */
export const COLLECTION_INTERVAL_MS = SEARCH_GATE.intervalMs;

/**
 * Looks up every card's printing in as few requests as Scryfall allows.
 * Cards Scryfall cannot find are absent from the map rather than an error: the
 * deck is still the deck, and the view shows what the file names instead.
 */
export async function fetchPrintings(
  cards: readonly Card[],
): Promise<Printings> {
  const wanted = [
    ...new Map(cards.map((c) => [printingKey(c.card), c.card])).values(),
  ];
  const found = new Map<string, Printing>();
  for (let i = 0; i < wanted.length; i += BATCH) {
    const batch = wanted.slice(i, i + BATCH);
    const response = await scryfallFetch(
      SEARCH_GATE,
      `${API}/cards/collection`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ identifiers: batch.map(identifier) }),
      },
    );
    if (!response.ok) throw new Error(`Scryfall answered ${response.status}`);
    for (const card of parseCollection(await response.json())) {
      const byPrinting = `${card.set}/${card.collector_number}`;
      const printing: Printing = {
        name: card.name,
        image: card.image,
        colorIdentity: card.colorIdentity,
        ...(card.prints ? { prints: card.prints } : {}),
      };
      found.set(byPrinting, printing);
      // A name lookup returns whichever printing Scryfall prefers, so it is
      // also keyed by name, front face included for double-faced cards.
      found.set(`name:${card.name.toLowerCase()}`, printing);
      const front = card.name.split(" // ")[0];
      if (front !== undefined)
        found.set(`name:${front.toLowerCase()}`, printing);
    }
  }
  return found;
}

interface CollectionCard {
  name: string;
  set: string;
  collector_number: string;
  image: string;
  colorIdentity: string[];
  prints?: string;
}

/** Scryfall's JSON is outside our types until it has been checked. */
function parseCollection(json: unknown): CollectionCard[] {
  const data = (json as { data?: unknown }).data;
  if (!Array.isArray(data))
    throw new Error("Scryfall collection response has no data");
  return data.flatMap((raw: unknown): CollectionCard[] => {
    const c = raw as Record<string, unknown>;
    const faces = Array.isArray(c.card_faces)
      ? (c.card_faces as Record<string, unknown>[])
      : [];
    const uris = (c.image_uris ?? faces[0]?.image_uris) as
      | Record<string, unknown>
      | undefined;
    const image = uris?.normal;
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
        collector_number: c.collector_number.toLowerCase(),
        image,
        colorIdentity: Array.isArray(c.color_identity)
          ? c.color_identity.filter((x): x is string => typeof x === "string")
          : [],
        ...(typeof c.prints_search_uri === "string"
          ? { prints: c.prints_search_uri }
          : {}),
      },
    ];
  });
}
