import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import {
  type Card,
  type Category,
  editDeckCards,
  loadDeckSync,
  parseDeck,
} from "../deck";
import { groupByCategory, packColumns } from "./layout";

let index = 0;
const card = (name: string, ...categories: string[]): Card => ({
  index: index++,
  card: { kind: "name", name },
  qty: 1,
  finish: "nonfoil",
  categories,
  place: "in-deck",
  inDeck: true,
});

const nameOf = (c: Card) => (c.card.kind === "name" ? c.card.name : "");

const categories: Category[] = [
  { name: "Commander", kind: "commander" },
  { name: "learnboard", kind: "sideboard" },
  { name: "tempo" },
  { name: "self-bounce" },
];

describe("grouping", () => {
  it("shows a card under every category it is in", () => {
    const groups = groupByCategory(
      categories,
      [
        card("Boomerang Basics", "learnboard", "self-bounce", "tempo"),
        card("Snap", "tempo"),
      ],
      nameOf,
    );
    const where = groups.filter((g) =>
      g.cards.some(
        (c) => c.card.kind === "name" && c.card.name === "Boomerang Basics",
      ),
    );
    expect(where.map((g) => g.category)).toEqual([
      "self-bounce",
      "tempo",
      "learnboard",
    ]);
  });

  it("puts the commander first and what is outside the deck last", () => {
    const groups = groupByCategory(
      categories,
      [
        card("Lesson", "learnboard"),
        card("Island"),
        card("Rashmi", "Commander"),
        card("Snap", "tempo"),
      ],
      nameOf,
    );
    expect(groups.map((g) => g.category)).toEqual([
      "Commander",
      "tempo",
      null,
      "learnboard",
    ]);
  });
});

describe("headers", () => {
  beforeAll(() => {
    loadDeckSync(
      readFileSync(
        new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url),
      ),
    );
  });

  // The commander's category sorts last by name, and Sol Ring is in two.
  const text = `cards = [
  { name = "Island", qty = 3, in = ["Lands"] },
  { name = "Rashmi and Ragavan", in = ["Zenith"] },
  { name = "Sol Ring", in = ["Ramp", "Artifacts"] },
  { name = "Arcane Signet", in = ["Ramp"] },
]

[categories]
Artifacts = {}
Lands = {}
Ramp = {}
Zenith = { type = "commander" }
`;

  const deck = (text: string) => {
    const parsed = parseDeck(text);
    if (parsed.kind !== "deck") throw new Error(parsed.message);
    return parsed;
  };
  const headers = (text: string) => {
    const parsed = deck(text);
    return groupByCategory(parsed.categories, parsed.cards, nameOf).map(
      (g) => [g.category, g.qty] as const,
    );
  };

  it("count a card in every header it is under, and once in the deck", () => {
    expect(headers(text)).toEqual([
      ["Zenith", 1],
      ["Artifacts", 1],
      ["Lands", 3],
      ["Ramp", 2],
    ]);
    expect(deck(text).total).toBe(6);
  });

  it("put the commander category first whatever it is called", () => {
    const columns = packColumns(
      groupByCategory(deck(text).categories, deck(text).cards, nameOf),
      () => 1,
      3,
    );
    expect(columns[0]?.[0]?.category).toBe("Zenith");
    expect(columns[0]?.[0]?.kind).toBe("commander");
  });

  it("both change when a drag moves a card", () => {
    const signet = deck(text).cards.find((c) => nameOf(c) === "Arcane Signet");
    if (!signet) throw new Error("no signet");
    const moved = editDeckCards(
      text,
      {
        kind: "move",
        to: { kind: "category", name: "Artifacts" },
        secondary: false,
      },
      [{ index: signet.index, from: "Ramp" }],
    ).text;
    expect(headers(moved)).toEqual([
      ["Zenith", 1],
      ["Artifacts", 2],
      ["Lands", 3],
      ["Ramp", 1],
    ]);
    expect(deck(moved).total).toBe(6);
  });
});

describe("packing", () => {
  it("fills the first row in order, then the shortest column", () => {
    const columns = packColumns(
      ["a", "b", "c", "d"],
      (x) => (x === "a" ? 1 : 5),
      3,
    );
    expect(columns).toEqual([["a", "d"], ["b"], ["c"]]);
  });
});
