import type { Card, Kind } from "../deck";
import { cardName, type Printings, printingKey } from "../scryfall";

/**
 * Archidekt's playtester takes a whole deck in its URL, which is how its own
 * sandbox opens one without saving it: `deck` is URI-encoded JSON, a card per
 * line of the deck. Read off archidekt.com's bundle (the sandbox's
 * "Playtester" action), not documented by Archidekt, so it can change under us.
 */
const PLAYTESTER = "https://archidekt.com/playtester-v2/sandbox";

/**
 * Archidekt answers 431 once a request's line and headers pass about 16KB
 * (measured: a 16,116-byte URL loads, 16,129 does not). Its own cookies count
 * against that too, so a URL under this can still be turned away; one over it
 * never loads.
 */
const MAX_URL = 16_116;

/**
 * JSON that keeps its brackets, colons and commas literal, which a query
 * allows, and escapes only what it must. Every card is `{"u":…}` in that
 * alphabet, so this is 89 bytes a card instead of encodeURIComponent's 109,
 * the difference between a 120-card deck fitting beside Archidekt's cookies
 * and a 431.
 */
function queryJson(value: unknown): string {
  return encodeURIComponent(JSON.stringify(value)).replace(
    /%(7B|7D|5B|5D|3A|2C)/g,
    (escaped) => decodeURIComponent(escaped),
  );
}

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
 * answer for a different deck, so it is refused by name instead; so is a deck
 * too long for Archidekt to accept in a URL.
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
  const url = `${PLAYTESTER}?deck=${queryJson(deck)}`;
  if (url.length > MAX_URL)
    return {
      kind: "refused",
      message: `Archidekt's playtester can't take ${deck.length} different cards in one link`,
    };
  return { kind: "url", url };
}
