import type { Card, Category } from "../deck";
import { groupByCategory } from "../deck/layout";

/**
 * The deck's cards as the stacks show them: category by category, and by name
 * within one. A card in several categories is met at its first stack only, so
 * stepping through the order visits every card once.
 */
export function displayOrder(
  categories: readonly Category[],
  cards: readonly Card[],
  nameOf: (card: Card) => string,
): number[] {
  const seen = new Set<number>();
  for (const group of groupByCategory(categories, cards, nameOf))
    for (const card of group.cards) seen.add(card.index);
  return [...seen];
}

/** Where `index` is in `order`, and the cards either side of it. */
export function neighbours(
  order: readonly number[],
  index: number,
): { prev: number | null; next: number | null; position: number } {
  const at = order.indexOf(index);
  return {
    prev: at > 0 ? (order[at - 1] ?? null) : null,
    next: at >= 0 ? (order[at + 1] ?? null) : null,
    position: at,
  };
}

/**
 * The card to show once `removed` has left the deck: the one after it in
 * `order`, else the one before, else none. Its index is as it will be in the
 * next text, where every card after the removed line moves up by one.
 */
export function afterRemoval(
  order: readonly number[],
  removed: number,
): number | null {
  const { prev, next } = neighbours(order, removed);
  const stay = next ?? prev;
  if (stay === null) return null;
  return stay > removed ? stay - 1 : stay;
}
