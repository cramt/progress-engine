import type { Card, Kind } from "../deck";
import { cardName, type Printings, printingKey } from "../scryfall";

/**
 * Archidekt's playtester takes a whole deck in its URL, which is how its own
 * sandbox opens one without saving it: `deck` is URI-encoded JSON, a card per
 * line of the deck. Read off archidekt.com's bundle (the sandbox's
 * "Playtester" action), not documented by Archidekt, so it can change under us.
 */
const PLAYTESTER = "https://archidekt.com/playtester-v2/sandbox";

/** Archidekt's zones: main deck, command zone, sideboard, attraction deck. */
type Zone = "m" | "c" | "s" | "a";

interface PlaytestCard {
  /** Scryfall id. */
  u: string;
  q: number;
  /** 1 for anything but nonfoil, as Archidekt's `modifier !== "Normal"`. */
  f: 0 | 1;
  c: Zone;
}

/**
 * Where a card starts a playtest, or `undefined` for a card the game never
 * sees. A companion begins in the sideboard, which is where the rules keep it.
 */
const ZONE: Record<Kind, Zone | undefined> = {
  "in-deck": "m",
  commander: "c",
  sideboard: "s",
  companion: "s",
  attractions: "a",
  "not-in-deck": undefined,
  maybeboard: undefined,
  "sticker-sheet": undefined,
};

export type Playtest =
  | { kind: "url"; url: string }
  | { kind: "refused"; message: string };

/**
 * The URL that opens the deck in Archidekt's playtester. A card Scryfall has
 * not resolved has no id to send, and playtesting a deck missing it would
 * answer for a different deck, so it is refused by name instead.
 */
export function playtestUrl(
  cards: readonly Card[],
  printings: Printings,
): Playtest {
  const deck: PlaytestCard[] = [];
  const unresolved: string[] = [];
  for (const card of cards) {
    const zone = ZONE[card.place];
    if (zone === undefined) continue;
    const id = printings.get(printingKey(card.card))?.id;
    if (id === undefined) {
      unresolved.push(cardName(card, printings));
      continue;
    }
    deck.push({
      u: id,
      q: card.qty,
      f: card.finish === "nonfoil" ? 0 : 1,
      c: zone,
    });
  }
  if (unresolved.length > 0)
    return {
      kind: "refused",
      message: `Can't playtest until Scryfall knows ${unresolved.join(", ")}`,
    };
  return {
    kind: "url",
    url: `${PLAYTESTER}?deck=${encodeURIComponent(JSON.stringify(deck))}`,
  };
}
