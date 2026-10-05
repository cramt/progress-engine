import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadDeckSync, parseDeck } from "../deck";
import type { Printings } from "../scryfall";
import { addByName } from "./addByName";

beforeAll(() => {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
});

const deck = `name = "Test"

cards = [
  { printing = "cmm/410", in = ["Ramp"] },  # Sol Ring
  { name = "Arcane Signet", in = ["Ramp"] },
  { name = "Counterspell", in = ["Maybe"] },
]

[categories]
Ramp = {}
Draw = {}
Maybe = { type = "maybeboard" }
`;

const printings: Printings = new Map([
  [
    "cmm/410",
    {
      id: "cmm/410",
      name: "Sol Ring",
      set: "cmm",
      num: "410",
      image: "",
      colorIdentity: [],
      typeLine: "",
    },
  ],
]);

function add(text: string, name: string, category: string | null) {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") throw new Error(parsed.message);
  return addByName(text, parsed.cards, printings, name, category);
}

/** The lines of `after` that are not in `before`, and the reverse. */
function diff(before: string, after: string) {
  const a = before.split("\n");
  const b = after.split("\n");
  return {
    added: b.filter((l) => !a.includes(l)),
    removed: a.filter((l) => !b.includes(l)),
  };
}

describe("adding a card by name", () => {
  it("gives a card the deck names by printing one more copy", () => {
    const next = add(deck, "Sol Ring", null);
    expect(diff(deck, next)).toEqual({
      added: [
        '  { printing = "cmm/410", qty = 2, in = ["Ramp"] },  # Sol Ring',
      ],
      removed: ['  { printing = "cmm/410", in = ["Ramp"] },  # Sol Ring'],
    });
  });

  it("gives the line already in the chosen category the copy", () => {
    const next = add(deck, "arcane signet", "Ramp");
    expect(diff(deck, next).added).toEqual([
      '  { name = "Arcane Signet", qty = 2, in = ["Ramp"] },',
    ]);
  });

  it("makes a new line when the card is in the deck, but not in that category", () => {
    const next = add(deck, "Sol Ring", "Draw");
    expect(diff(deck, next)).toEqual({
      added: ['  { name = "Sol Ring", in = ["Draw"] },'],
      removed: [],
    });
  });

  it("puts a new card automatically in no category", () => {
    const next = add(deck, "Brainstorm", null);
    expect(diff(deck, next)).toEqual({
      added: ['  { name = "Brainstorm" },'],
      removed: [],
    });
  });

  it("automatically bumps a card held only outside the deck", () => {
    const next = add(deck, "Counterspell", null);
    expect(diff(deck, next).added).toEqual([
      '  { name = "Counterspell", qty = 2, in = ["Maybe"] },',
    ]);
  });
});
