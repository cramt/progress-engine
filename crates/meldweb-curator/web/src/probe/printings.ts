import { addCard, type Card, setCardQty } from "../deck";
import { imageUris, printingId } from "../scryfall";
import { API, SEARCH_GATE, scryfallFetch } from "../scryfallQueue";

/** A scanned printing as the deck names it, with Scryfall's picture of it. */
export interface ScannedPrinting {
  set: string;
  num: string;
  name: string;
  image: string | null;
}

/**
 * Scryfall's printing for each `scryfall_id` the probe found, in one request
 * (the probe's candidates are at most eleven per card, well under the 75 a
 * collection lookup takes). Delver names editions, not set codes, so this is
 * how a scan becomes a `set/num` the deck can hold. Ids Scryfall does not
 * know are absent.
 */
export async function printingsById(
  ids: readonly string[],
): Promise<Map<string, ScannedPrinting>> {
  const found = new Map<string, ScannedPrinting>();
  const wanted = [...new Set(ids)];
  for (let i = 0; i < wanted.length; i += 75) {
    const response = await scryfallFetch(
      SEARCH_GATE,
      `${API}/cards/collection`,
      {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          identifiers: wanted.slice(i, i + 75).map((id) => ({ id })),
        }),
      },
    );
    if (!response.ok) throw new Error(`Scryfall answered ${response.status}`);
    const data = ((await response.json()) as { data?: unknown }).data;
    if (!Array.isArray(data)) continue;
    for (const raw of data as Record<string, unknown>[]) {
      const { id, set, collector_number: num, name } = raw;
      if (
        typeof id !== "string" ||
        typeof set !== "string" ||
        typeof num !== "string" ||
        typeof name !== "string"
      )
        continue;
      const image = imageUris(raw)?.normal;
      found.set(id, {
        set: set.toLowerCase(),
        num: num.toLowerCase(),
        name,
        image: typeof image === "string" ? image : null,
      });
    }
  }
  return found;
}

/**
 * The deck text with one more copy of this printing: a copy more on the line
 * that already names it, in the deck rather than a side category if there is
 * one, else a new line with no category - Archidekt's *Automatic*.
 */
export function addPrinting(
  text: string,
  cards: readonly Card[],
  printing: { set: string; num: string; name: string },
): string {
  const id = printingId(printing);
  const held = cards.filter(
    (c) => c.card.kind === "printing" && printingId(c.card) === id,
  );
  const line = held.find((c) => c.inDeck) ?? held[0];
  if (line) return setCardQty(text, line.index, line.qty + 1);
  return addCard(text, {
    kind: "printing",
    set: printing.set,
    num: printing.num,
    name: printing.name,
  });
}
