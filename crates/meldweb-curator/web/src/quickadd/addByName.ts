import { addCard, type Card, setCardQty } from "../deck";
import { type Printings, printingKey } from "../scryfall";

/** The card's name: as the file writes it, or as Scryfall names its printing. */
export function resolvedName(card: Card, printings: Printings): string | null {
  return card.card.kind === "name"
    ? card.card.name
    : (printings.get(printingKey(card.card))?.name ?? null);
}

/** One name for another, whole or by front face, whatever the case. */
function sameCard(a: string, b: string): boolean {
  const front = (n: string) => (n.split(" // ")[0] ?? n).toLowerCase();
  return a.toLowerCase() === b.toLowerCase() || front(a) === front(b);
}

/**
 * The deck text with one more copy of the card called `name`, the way quick
 * add and the search's `+` add one.
 *
 * `category` is where it goes; `null` is Archidekt's *Automatic*. A card the
 * deck already holds gains a copy rather than a second line, whether the file
 * names it or one of its printings: with a category, the line already in
 * exactly that category; automatically, the first line that is in the deck
 * (else the first line at all). Otherwise it becomes a new line by name, in
 * `category` or in none.
 */
export function addByName(
  text: string,
  cards: readonly Card[],
  printings: Printings,
  name: string,
  category: string | null,
): string {
  const held = cards.filter((c) => {
    const n = resolvedName(c, printings);
    return n !== null && sameCard(n, name);
  });
  const line =
    category === null
      ? (held.find((c) => c.inDeck) ?? held[0])
      : held.find(
          (c) => c.categories.length === 1 && c.categories[0] === category,
        );
  if (line) return setCardQty(text, line.index, line.qty + 1);
  return addCard(
    text,
    { kind: "name", name },
    category === null ? [] : [category],
  );
}
