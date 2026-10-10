import { describe, expect, it } from "vitest";
import type { PrintingOption } from "../card/prints";
import type { Card } from "../deck";
import type { PriceBook } from "../prices";
import { atCheapest, breakdown, cheapest } from "./cost";

let index = 0;
const card = (name: string, extra: Partial<Card> = {}): Card => ({
  index: index++,
  card: { kind: "name", name },
  qty: 1,
  finish: "nonfoil",
  categories: [],
  place: "in-deck",
  inDeck: true,
  ...extra,
});

const nameOf = (c: Card) => (c.card.kind === "name" ? c.card.name : "");

const prices: PriceBook = new Map([
  ["name:the one ring", { eur: { nonfoil: 60, foil: 90 }, usd: {} }],
  ["name:sol ring", { eur: { nonfoil: 1.5 }, usd: {} }],
  ["name:arcane signet", { eur: { nonfoil: 0.4 }, usd: {} }],
  ["name:lightning bolt", { eur: { nonfoil: 0.5 }, usd: {} }],
  ["name:jeweled lotus", { eur: { nonfoil: 10 }, usd: {} }],
]);

describe("a deck's cost", () => {
  it("lists each line dearest first by one copy, and totals every copy", () => {
    const b = breakdown(
      [
        card("Sol Ring"),
        card("Lightning Bolt", { qty: 4 }),
        card("The One Ring", { finish: "foil" }),
        card("Jeweled Lotus"),
        card("Arcane Signet"),
      ],
      prices,
      "eur",
      nameOf,
    );
    expect(b.lines.map((l) => [nameOf(l.card), l.each])).toEqual([
      // A foil is priced as one.
      ["The One Ring", 90],
      ["Jeweled Lotus", 10],
      ["Sol Ring", 1.5],
      // Four copies still sort by what one costs.
      ["Lightning Bolt", 0.5],
      ["Arcane Signet", 0.4],
    ]);
    expect(b.total).toBeCloseTo(103.9);
  });

  it("counts only the deck, and owns up to what it could not price", () => {
    const b = breakdown(
      [
        card("The One Ring", { inDeck: false, place: "maybeboard", qty: 2 }),
        card("Sol Ring", { finish: "etched" }),
        card("Arcane Signet"),
      ],
      prices,
      "eur",
      nameOf,
    );
    expect(b.outside).toBe(2);
    expect(b.unpriced.map(nameOf)).toEqual(["Sol Ring"]);
    expect(b.total).toBe(0.4);
  });
});

const printing = (
  set: string,
  prices: PrintingOption["prices"],
  facts: Record<string, unknown> = {},
): PrintingOption => ({
  id: set,
  name: "Sol Ring",
  set,
  num: "1",
  setName: set,
  released: "2020-01-01",
  finishes: ["nonfoil", "foil"],
  prices,
  facts,
});

describe("a card's cheapest printing", () => {
  it("prices the deck with each line no dearer than its cheapest", () => {
    const ring = card("The One Ring");
    const signet = card("Arcane Signet");
    const lotus = card("Jeweled Lotus", { finish: "etched" });
    const b = breakdown([ring, signet, lotus], prices, "eur", nameOf);
    const cheap = printing("ltr", { eur: { nonfoil: 45 }, usd: {} });
    const dear = printing("sld", { eur: { nonfoil: 80 }, usd: {} });
    const lotusPrint = printing("cmr", { eur: { nonfoil: 9 }, usd: {} });
    const of = new Map([
      [ring, { printing: cheap, finish: "nonfoil" as const, each: 45 }],
      // A cheapest dearer than the line's own (prices move) saves nothing.
      [signet, { printing: dear, finish: "nonfoil" as const, each: 80 }],
      // A line with no price of its own is not in the total it compares with.
      [lotus, { printing: lotusPrint, finish: "nonfoil" as const, each: 9 }],
    ]);
    expect(atCheapest(b, (c) => of.get(c))).toBeCloseTo(45 + 0.4);
  });

  it("is the cheapest paper copy in any finish", () => {
    const best = cheapest(
      [
        printing("lea", { eur: { nonfoil: 900 }, usd: {} }),
        printing("cmr", { eur: { nonfoil: 1.2, foil: 0.9 }, usd: {} }),
        printing("prm", { eur: { nonfoil: 0.1 }, usd: {} }, { digital: true }),
        printing(
          "o90p",
          { eur: { nonfoil: 0.05 }, usd: {} },
          { oversized: true },
        ),
      ],
      "eur",
    );
    expect(best && [best.printing.set, best.finish, best.each]).toEqual([
      "cmr",
      "foil",
      0.9,
    ]);
  });

  it("is nothing when no printing has a price in the currency", () => {
    expect(
      cheapest([printing("cmr", { eur: { nonfoil: 1 }, usd: {} })], "usd"),
    ).toBeUndefined();
  });
});
