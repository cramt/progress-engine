import { bestPrinting, type LoadPrintings } from "../card/bestPrinting";
import { parseDeck, setCardPrinting } from "../deck";

/** What each name-only card in a deck resolves to; absent when Scryfall has none or failed. */
export type Resolved = ReadonlyMap<string, { set: string; num: string }>;

/** The names the deck holds by name only, each once. */
export function namesOnly(text: string): string[] {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") return [];
  const names = parsed.cards.flatMap(({ card }) =>
    card.kind === "name" ? [card.name] : [],
  );
  return [...new Set(names)];
}

/**
 * Each name-only card's printing, as an added card would get it: its pin, else
 * the one the rules rank first. A card Scryfall cannot answer for is left out,
 * so its line stays by name.
 */
export async function resolveNames(
  names: readonly string[],
  settings: string | null,
  load?: LoadPrintings,
): Promise<Resolved> {
  const answers = await Promise.allSettled(
    names.map(
      async (name) => [name, await bestPrinting(name, settings, load)] as const,
    ),
  );
  const resolved = new Map<string, { set: string; num: string }>();
  for (const a of answers)
    if (a.status === "fulfilled" && a.value[1])
      resolved.set(a.value[0], a.value[1]);
  return resolved;
}

/**
 * `text` with every line still by a resolved name put on its printing. Matched
 * by name, not by line, since the deck may have been edited while Scryfall
 * answered; a line named or reprinted since is left as it is.
 */
export function applyResolved(
  text: string,
  resolved: Resolved,
): { text: string; count: number } {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") return { text, count: 0 };
  let out = text;
  let count = 0;
  for (const { index, card } of parsed.cards) {
    if (card.kind !== "name") continue;
    const p = resolved.get(card.name);
    if (!p) continue;
    out = setCardPrinting(out, index, p.set, p.num);
    count++;
  }
  return { text: out, count };
}
