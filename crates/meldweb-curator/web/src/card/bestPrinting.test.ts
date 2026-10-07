import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { addCard, loadDeckSync, newDeck, parseDeck, setCardQty } from "../deck";
import { addedLine, bestPrinting } from "./bestPrinting";
import { parsePrintsPage } from "./prints";

beforeAll(() => {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
});

const printing = (set: string, num: string, extra = {}) => ({
  object: "card",
  id: `${set}/${num}`,
  name: "Sol Ring",
  type_line: "Artifact",
  oracle_text: "{T}: Add {C}{C}.",
  mana_cost: "{1}",
  cmc: 1,
  colors: [],
  color_identity: [],
  lang: "en",
  layout: "normal",
  set,
  set_name: set.toUpperCase(),
  collector_number: num,
  released_at: "2021-01-01",
  games: ["paper"],
  border_color: "black",
  frame: "2015",
  ...extra,
});

const solRings = () =>
  Promise.resolve(
    parsePrintsPage({
      data: [
        printing("ltc", "1", { promo_types: ["universesbeyond"] }),
        printing("c21", "263"),
        printing("pza", "1", { digital: true, games: ["arena"] }),
      ],
    }).printings,
  );

const cards = (text: string) => {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") throw new Error(parsed.message);
  return parsed.cards;
};

describe("the printing an added card gets", () => {
  it("is its pin, without asking Scryfall", async () => {
    const pins = [{ name: "Sol Ring", set: "2xm", num: "270" }];
    const never = () => Promise.reject(new Error("asked"));
    expect(await bestPrinting("sol ring", null, pins, never)).toEqual({
      set: "2xm",
      num: "270",
    });
  });

  it("is the first the rules rank, when it has no pin", async () => {
    // The default rules sink Universes Beyond and digital printings.
    expect(await bestPrinting("Sol Ring", null, [], solRings)).toEqual({
      set: "c21",
      num: "263",
    });
  });

  it("is asked for only when the add made a line", () => {
    const empty = newDeck("Test", "commander");
    const one = addCard(empty, { kind: "name", name: "Sol Ring" });
    expect(addedLine(cards(empty), cards(one), "Sol Ring")).toBe(0);
    // A second copy is a copy on the line, not a line.
    const two = setCardQty(one, 0, 2);
    expect(addedLine(cards(one), cards(two), "Sol Ring")).toBeNull();
  });
});
