/**
 * What the custom card form's buttons do to the text of a field. The form's
 * fields themselves, and writing them back into the file, are Mirage
 * Mockery's (`formOf`, `writeForm`); what a valid card is stays its check.
 */

/** `{2}{U}{U/P}` as `["2", "U", "U/P"]`; text outside braces is dropped. */
export function costSymbols(cost: string): string[] {
  return [...cost.matchAll(/\{([^}]*)\}/g)].map((m) => m[1] as string);
}

function costText(symbols: readonly string[]): string {
  return symbols.map((s) => `{${s}}`).join("");
}

const GENERIC = /^\d+$/;

/** The generic part of a cost: the sum of its number symbols. */
export function genericOf(cost: string): number {
  return costSymbols(cost)
    .filter((s) => GENERIC.test(s))
    .reduce((n, s) => n + Number(s), 0);
}

/**
 * The cost with its generic part `n`, written first as Oracle text writes it
 * (after any `{X}`), and none at all for 0.
 */
export function withGeneric(cost: string, n: number): string {
  const rest = costSymbols(cost).filter((s) => !GENERIC.test(s));
  const xs = rest.filter((s) => s === "X");
  const others = rest.filter((s) => s !== "X");
  return costText([...xs, ...(n > 0 ? [String(n)] : []), ...others]);
}

/** The cost with `symbol` added, an `{X}` at the front with the others. */
export function addSymbol(cost: string, symbol: string): string {
  const all = costSymbols(cost);
  return costText(symbol === "X" ? [symbol, ...all] : [...all, symbol]);
}

/** The cost without its `index`th symbol. */
export function dropSymbol(cost: string, index: number): string {
  return costText(costSymbols(cost).filter((_, i) => i !== index));
}

const WUBRG = ["W", "U", "B", "R", "G"] as const;

/** The colour indicator with `color` in it or out of it, kept in WUBRG order. */
export function toggleColor(indicator: string, color: string): string {
  const has = new Set(indicator.toUpperCase());
  if (has.has(color)) has.delete(color);
  else has.add(color);
  return WUBRG.filter((c) => has.has(c)).join("");
}

/** The words before the dash, in the order a type line puts them. */
export const SUPERTYPES = ["Legendary", "Basic", "Snow", "World"] as const;
export const CARD_TYPES = [
  "Kindred",
  "Artifact",
  "Enchantment",
  "Creature",
  "Land",
  "Planeswalker",
  "Battle",
  "Instant",
  "Sorcery",
] as const;
const ORDER: readonly string[] = [...SUPERTYPES, ...CARD_TYPES];
const DASH = " — ";

/** A type line's words before the dash, and its subtypes after it. */
export function splitType(typeLine: string): {
  types: string[];
  subtypes: string;
} {
  const at = typeLine.indexOf("—");
  const head = at < 0 ? typeLine : typeLine.slice(0, at);
  return {
    types: head.split(/\s+/).filter(Boolean),
    subtypes: at < 0 ? "" : typeLine.slice(at + 1).trim(),
  };
}

export function joinType(types: readonly string[], subtypes: string): string {
  const head = types.join(" ");
  return subtypes.trim() ? `${head}${DASH}${subtypes.trim()}` : head;
}

/**
 * The type line with `word` in it or out of it. A word the buttons know goes
 * where Oracle text puts it; words they don't know keep their places.
 */
export function toggleType(typeLine: string, word: string): string {
  const { types, subtypes } = splitType(typeLine);
  if (types.includes(word))
    return joinType(
      types.filter((t) => t !== word),
      subtypes,
    );
  const rank = ORDER.indexOf(word);
  const before = types.findIndex((t) => {
    const r = ORDER.indexOf(t);
    return r > rank;
  });
  const next = [...types];
  next.splice(before < 0 ? next.length : before, 0, word);
  return joinType(next, subtypes);
}

/** `text` with `insert` put in place of `[start, end)`, and where the caret goes. */
export function insertAt(
  text: string,
  insert: string,
  start: number,
  end: number,
): { text: string; caret: number } {
  return {
    text: text.slice(0, start) + insert + text.slice(end),
    caret: start + insert.length,
  };
}
