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
export function cardName(card: Card, printings: Printings): string {
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

type Identifier = { set: string; collector_number: string } | { name: string };

function identifier(ref: CardRef): Identifier {
  return ref.kind === "printing"
    ? { set: ref.set, collector_number: ref.num }
    : { name: ref.name };
}

// Scryfall's limit per request.
const BATCH = 75;

/**
 * Looks up every card's printing in as few requests as Scryfall allows.
 * Cards Scryfall cannot find are absent from the map rather than an error: the
 * deck is still the deck, and the view shows what the file names instead.
 * A batch still queued when `signal` aborts is never sent.
 */
export async function fetchPrintings(
  cards: readonly Card[],
  signal?: AbortSignal,
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
        ...(signal ? { signal } : {}),
      },
    );
    if (!response.ok) throw new Error(`Scryfall answered ${response.status}`);
    for (const card of parseCollection(await response.json())) {
      const byPrinting = printingId({
        set: card.set,
        num: card.collector_number,
      });
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
