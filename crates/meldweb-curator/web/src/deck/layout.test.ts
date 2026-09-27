import { describe, expect, it } from "vitest";
import type { Card, Category } from "../deck";
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
