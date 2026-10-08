import type { OwnedCard, Place } from "../collection";

/** Owned cards not put in any place yet. */
export const UNSORTED = "Unsorted";

export interface Section {
  /** The place, or null for unsorted. */
  place: Place | null;
  cards: OwnedCard[];
  qty: number;
}

/** Unsorted copies wait to be put away, so they come first. */
export function sections(
  places: readonly Place[],
  cards: readonly OwnedCard[],
  nameOf: (card: OwnedCard) => string,
  filter = "",
  order: (a: OwnedCard, b: OwnedCard) => number = (a, b) =>
    nameOf(a).localeCompare(nameOf(b)) || a.index - b.index,
): Section[] {
  const wanted = filter.trim().toLowerCase();
  const shown = wanted
    ? cards.filter((c) => nameOf(c).toLowerCase().includes(wanted))
    : cards;
  const section = (place: Place | null): Section => {
    const here = shown
      .filter((c) => (c.at ?? null) === (place?.name ?? null))
      .toSorted(order);
    return { place, cards: here, qty: here.reduce((n, c) => n + c.qty, 0) };
  };
  const all = [section(null), ...places.map(section)];
  // Every place shows, empty or not, so there is somewhere to move cards to;
  // a filter shows only where it matched.
  return all.filter((s) =>
    wanted || s.place === null ? s.cards.length > 0 : true,
  );
}
