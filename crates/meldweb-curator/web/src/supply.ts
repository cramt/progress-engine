/**
 * Where one deck's cards come from, `chip-decklist`'s `supply` module: the
 * collection's copies each line takes, what the deck's place holds besides,
 * and which friends hold what is short. Load `deck.ts` first; this module
 * shares its wasm.
 */
import type { FriendFile, ParsedSupply } from "./deck.gen";
import {
  collection_move_some,
  read_supply,
  set_friends,
} from "./wasm/pkg/meldweb_wasm.js";

export type {
  FriendFile,
  FriendHas,
  ParsedSupply,
  SupplyCopies,
  SupplyExtra,
  SupplyLine,
  SupplyShort,
  Whereabouts,
} from "./deck.gen";

/**
 * The deck `stem`, its `text`, made of `collection`'s copies, and what
 * each of `friends` holds of what is still short. `names` names the
 * printings the files hold, as for an add.
 */
export function readSupply(
  stem: string,
  text: string,
  collection: string,
  friends: readonly FriendFile[],
  names: Readonly<Record<string, string>> = {},
): ParsedSupply {
  return JSON.parse(
    read_supply(
      stem,
      text,
      collection,
      JSON.stringify(friends),
      JSON.stringify(names),
    ),
  ) as ParsedSupply;
}

/** `qty` copies of each collection `line` moved to `to`, as one edit. */
export function moveSome(
  text: string,
  moves: readonly { line: number; qty: number }[],
  to: string | null,
): string {
  return collection_move_some(
    text,
    Uint32Array.from(moves.map((m) => m.line)),
    Uint32Array.from(moves.map((m) => m.qty)),
    to ?? "",
  );
}

/** `settings`, the repo's `meldweb.toml` or none, with these friends. */
export function setFriends(
  settings: string | null,
  friends: readonly string[],
): string {
  return set_friends(settings ?? undefined, JSON.stringify(friends));
}
