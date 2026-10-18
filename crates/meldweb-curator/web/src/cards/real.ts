/**
 * The real cards a custom one is checked against (ADR-0039). The CLI reads
 * the whole card index; the page asks Scryfall's search for the two things
 * the refusal looks at: a card by the same name, and the cards at the same
 * cost whose text holds the custom card's longest word, which any real card
 * with the same text has too. The comparing is `mockery-core`'s.
 */
import type { Real } from "../mockery";
import { API, SEARCH_GATE, scryfallFetch } from "../scryfallQueue";

interface Face {
  name: string;
  mana_cost?: string;
  oracle_text?: string;
}
interface ScryCard extends Face {
  card_faces?: Face[];
}
interface Page {
  data: ScryCard[];
  has_more: boolean;
  next_page?: string;
}

/** Every card a search finds, none for a search that finds nothing. */
async function search(query: string, pages = 6): Promise<ScryCard[]> {
  const out: ScryCard[] = [];
  let url: string | undefined =
    `${API}/cards/search?unique=cards&q=${encodeURIComponent(query)}`;
  for (let i = 0; url && i < pages; i++) {
    const r = await scryfallFetch(SEARCH_GATE, url);
    if (r.status === 404) return out;
    if (!r.ok) throw new Error(`Scryfall's search answered ${r.status}`);
    const page = (await r.json()) as Page;
    out.push(...page.data);
    url = page.has_more ? page.next_page : undefined;
  }
  return out;
}

const quoted = (s: string) => `"${s.replace(/"/g, '\\"')}"`;

/** Each face or whole card that costs exactly `cost`, as a `Real`. */
export function atCost(cards: readonly ScryCard[], cost: string): Real[] {
  return cards.flatMap((c) =>
    [c, ...(c.card_faces ?? [])]
      .filter((f) => (f.mana_cost ?? "") === cost && f.oracle_text)
      .map((f) => ({ name: f.name, oracle: f.oracle_text as string })),
  );
}

export async function realCandidates(card: {
  name: string;
  cost: string;
  word?: string;
}): Promise<{ named: boolean; sameCost: Real[] }> {
  const [named, sameCost] = await Promise.all([
    search(`!${quoted(card.name)}`, 1).then((c) => c.length > 0),
    card.word
      ? search(
          card.cost
            ? `mana=${card.cost} o:${quoted(card.word)}`
            : `o:${quoted(card.word)}`,
        ).then((c) => atCost(c, card.cost))
      : Promise.resolve([]),
  ]);
  return { named, sameCost };
}
