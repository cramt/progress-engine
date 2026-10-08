import { describe, expect, it } from "vitest";
import type { Card, Category } from "../deck";
import { displayOrder, neighbours } from "./order";

const card = (index: number, name: string, categories: string[]): Card => ({
  index,
  card: { kind: "name", name },
  qty: 1,
  finish: "nonfoil",
  categories,
  place: "in-deck",
  inDeck: true,
});

const categories: Category[] = [
  { name: "Ramp" },
  { name: "Commander", kind: "commander" },
  { name: "Maybeboard", kind: "maybeboard" },
  { name: "Draw" },
];
const cards = [
  card(0, "Sol Ring", ["Ramp"]),
  card(1, "Rashmi", ["Commander"]),
  card(2, "Brainstone", ["Draw", "Ramp"]),
  card(3, "Arcane Signet", ["Ramp"]),
  card(4, "Opt", ["Maybeboard"]),
  card(5, "Island", []),
];
const nameOf = (c: Card) => (c.card.kind === "name" ? c.card.name : "");

describe("display order", () => {
  it("is the stacks' order: commander, then categories by name, by card name within, the board last", () => {
    // Draw: Brainstone. Ramp: Arcane Signet, Brainstone (met already), Sol Ring.
    // Uncategorized: Island. Maybeboard: Opt.
    expect(displayOrder(categories, cards, nameOf)).toEqual([1, 2, 3, 0, 5, 4]);
  });

  it("steps to the cards either side, and has none past the ends", () => {
    const order = [1, 2, 3, 0, 5, 4];
    expect(neighbours(order, 1)).toEqual({ prev: null, next: 2, position: 0 });
    expect(neighbours(order, 0)).toEqual({ prev: 3, next: 5, position: 3 });
    expect(neighbours(order, 4)).toEqual({ prev: 5, next: null, position: 5 });
  });
});
