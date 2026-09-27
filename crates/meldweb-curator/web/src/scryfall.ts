import type { Entry } from "./decklist";

/** What the editor needs of a Scryfall card: a picture of it. */
export interface Printing {
  name: string;
  image: string;
}

/** An entry's printing, keyed the way it was asked for. */
export type Printings = ReadonlyMap<string, Printing>;

export function printingKey(e: Pick<Entry, "name" | "set" | "num">): string {
  return e.set !== undefined && e.num !== undefined
    ? `${e.set.toLowerCase()}/${e.num.toLowerCase()}`
    : `name:${e.name.toLowerCase()}`;
}

type Identifier = { set: string; collector_number: string } | { name: string };

function identifier(e: Entry): Identifier {
  return e.set !== undefined && e.num !== undefined
    ? { set: e.set, collector_number: e.num }
    : { name: e.name };
}

// Scryfall's limit per request.
const BATCH = 75;

/**
 * Looks up every entry's printing in as few requests as Scryfall allows.
 * Cards Scryfall cannot find are absent from the map rather than an error: the
 * deck is still the deck, and the view shows the name instead of the image.
 */
export async function fetchPrintings(
  entries: readonly Entry[],
): Promise<Printings> {
  const wanted = [...new Map(entries.map((e) => [printingKey(e), e])).values()];
  const found = new Map<string, Printing>();
  for (let i = 0; i < wanted.length; i += BATCH) {
    // Scryfall asks for 50-100 ms between requests.
    if (i > 0) await new Promise((r) => setTimeout(r, 100));
    const batch = wanted.slice(i, i + BATCH);
    const response = await fetch("https://api.scryfall.com/cards/collection", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ identifiers: batch.map(identifier) }),
    });
    if (!response.ok) throw new Error(`Scryfall answered ${response.status}`);
    for (const card of parseCollection(await response.json())) {
      const byPrinting = `${card.set}/${card.collector_number}`;
      const printing = { name: card.name, image: card.image };
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
      },
    ];
  });
}
