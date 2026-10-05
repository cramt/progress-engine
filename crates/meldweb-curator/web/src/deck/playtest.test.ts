import { describe, expect, it } from "vitest";
import type { Card, CardRef, Finish, Kind } from "../deck";
import type { Printing, Printings } from "../scryfall";
import { playtestUrl } from "./playtest";

const card = (
  index: number,
  ref: CardRef,
  place: Kind,
  extra: { qty?: number; finish?: Finish } = {},
): Card => ({
  index,
  card: ref,
  qty: extra.qty ?? 1,
  finish: extra.finish ?? "nonfoil",
  categories: [],
  place,
  inDeck: place === "in-deck" || place === "commander",
});

const printing = (id: string, name: string): Printing => ({
  id,
  name,
  set: "",
  num: "",
  image: "",
  colorIdentity: [],
  typeLine: "",
});

/** The deck the URL carries, decoded the way Archidekt's playtester reads it. */
function sent(url: string): unknown {
  const deck = new URL(url).searchParams.get("deck");
  if (deck === null) throw new Error(`no deck in ${url}`);
  return JSON.parse(deck);
}

describe("opening a deck in archidekt's playtester", () => {
  const printings: Printings = new Map([
    ["sld/1208", printing("esika", "Esika, God of the Tree")],
    ["name:forest", printing("forest", "Forest")],
    ["cmm/410", printing("sol-ring", "Sol Ring")],
    ["thb/226", printing("lurrus", "Lurrus of the Dream-Den")],
    ["unf/200", printing("attraction", "Balloon Stand")],
    ["name:maybe", printing("maybe", "Maybe")],
  ]);

  it("sends every card the game sees, to the zone it starts in", () => {
    const playtest = playtestUrl(
      [
        card(0, { kind: "printing", set: "sld", num: "1208" }, "commander"),
        card(1, { kind: "name", name: "Forest" }, "in-deck", { qty: 7 }),
        card(2, { kind: "printing", set: "CMM", num: "410" }, "in-deck", {
          finish: "etched",
        }),
        card(3, { kind: "printing", set: "thb", num: "226" }, "companion"),
        card(4, { kind: "printing", set: "unf", num: "200" }, "attractions"),
        card(5, { kind: "name", name: "Maybe" }, "maybeboard"),
      ],
      printings,
    );
    if (playtest.kind !== "url") throw new Error(playtest.message);
    expect(playtest.url).toMatch(
      /^https:\/\/archidekt\.com\/playtester-v2\/sandbox\?deck=/,
    );
    expect(sent(playtest.url)).toEqual([
      { u: "esika", q: 1, f: 0, c: "c" },
      { u: "forest", q: 7, f: 0, c: "m" },
      { u: "sol-ring", q: 1, f: 1, c: "m" },
      { u: "lurrus", q: 1, f: 0, c: "s" },
      { u: "attraction", q: 1, f: 0, c: "a" },
    ]);
  });

  it("refuses a deck with a card scryfall has not resolved, naming it", () => {
    expect(
      playtestUrl(
        [
          card(0, { kind: "name", name: "Forest" }, "in-deck"),
          card(1, { kind: "name", name: "Mox Jasper" }, "in-deck"),
        ],
        printings,
      ),
    ).toEqual({
      kind: "refused",
      message: "Can't playtest until Scryfall knows Mox Jasper",
    });
  });

  it("does not wait on a card that never enters the game", () => {
    const playtest = playtestUrl(
      [card(0, { kind: "name", name: "Unknown" }, "maybeboard")],
      printings,
    );
    expect(playtest.kind).toBe("url");
  });
});
