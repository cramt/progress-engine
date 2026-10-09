import { viaCopyEach } from "../copy";
import { imageUris } from "../scryfall";
import { cachedMany } from "../scryfallCache";
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
export function printingsById(
  ids: readonly string[],
): Promise<ReadonlyMap<string, ScannedPrinting>> {
  return viaCopyEach(
    ids,
    (id) => id,
    async (copy, ids) => {
      const found = await copy.lookup(ids.map((id) => ({ kind: "id", id })));
      const out = new Map<string, ScannedPrinting>();
      ids.forEach((id, i) => {
        const f = found[i];
        if (f)
          out.set(id, {
            set: f.set,
            num: f.num,
            name: f.name,
            image: f.image ?? null,
          });
      });
      return out;
    },
    (ids) => cachedMany(["scryfall", "by-id"], ids, (id) => id, lookUp),
  );
}

async function lookUp(
  wanted: readonly string[],
): Promise<Map<string, ScannedPrinting>> {
  const found = new Map<string, ScannedPrinting>();
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
        num,
        name,
        image: typeof image === "string" ? image : null,
      });
    }
  }
  return found;
}
