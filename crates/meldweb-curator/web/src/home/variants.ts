import type { DeckEntry } from "../github/decks";

/**
 * Decks in name order, each deck's variants straight after it, so a budget
 * build sits beside the deck it came from. A variant whose parent is gone,
 * or is itself a variant, stands in name order on its own.
 */
export function familyOrder(decks: readonly DeckEntry[]): DeckEntry[] {
  const byName = [...decks].sort((a, b) => a.name.localeCompare(b.name));
  const paths = new Set(decks.map((d) => d.path));
  const childrenOf = new Map<string, DeckEntry[]>();
  for (const d of byName) {
    if (d.variantOf && d.variantOf !== d.path && paths.has(d.variantOf)) {
      childrenOf.set(d.variantOf, [...(childrenOf.get(d.variantOf) ?? []), d]);
    }
  }
  const placed = new Set<string>();
  const out: DeckEntry[] = [];
  const place = (d: DeckEntry) => {
    // A cycle of variants is a hand-edited file; each deck is placed once.
    if (placed.has(d.path)) return;
    placed.add(d.path);
    out.push(d);
    for (const child of childrenOf.get(d.path) ?? []) place(child);
  };
  for (const d of byName) {
    const parented = d.variantOf && childrenOf.get(d.variantOf)?.includes(d);
    if (!parented) place(d);
  }
  // What a cycle left unplaced, in name order.
  for (const d of byName) place(d);
  return out;
}
