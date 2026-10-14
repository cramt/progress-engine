import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadDeckSync, parseDeck } from "../deck";
import { pin_printing } from "../wasm/pkg/meldweb_wasm.js";
import { applyResolved, namesOnly, resolveNames } from "./resolveNames";

beforeAll(() => {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
});

const DECK = `cards = [
  { printing = "sld/2417" },  # Sol Ring
  { name = "Mind Stone" },
  { name = "Island", qty = 3 },
  { name = "Unheard Of" },
]
`;

const refs = (text: string) => {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") throw new Error(parsed.message);
  return parsed.cards.map((c) => c.card);
};

describe("a deck opened with cards by name", () => {
  it("asks once for each name-only card", () => {
    expect(
      namesOnly(`${DECK.slice(0, -2)}  { name = "Island" },\n]\n`),
    ).toEqual(["Mind Stone", "Island", "Unheard Of"]);
  });

  it("puts each on its pin, leaving what Scryfall has no printing of", async () => {
    const settings = pin_printing(undefined, "Mind Stone", "cmm", "389");
    const resolved = await resolveNames(
      namesOnly(DECK),
      settings,
      async (name) => {
        if (name === "Unheard Of") return [];
        throw new Error("Scryfall is out");
      },
    );
    const { text, count } = applyResolved(DECK, resolved);
    expect(count).toBe(1);
    expect(refs(text)).toEqual([
      { kind: "printing", set: "sld", num: "2417" },
      { kind: "printing", set: "cmm", num: "389" },
      { kind: "name", name: "Island" },
      { kind: "name", name: "Unheard Of" },
    ]);
  });

  it("leaves a line reprinted while Scryfall answered", () => {
    const edited = DECK.replace(
      '{ name = "Mind Stone" }',
      '{ printing = "c21/245" }',
    );
    const { count } = applyResolved(
      edited,
      new Map([["Mind Stone", { set: "cmm", num: "389" }]]),
    );
    expect(count).toBe(0);
  });
});
