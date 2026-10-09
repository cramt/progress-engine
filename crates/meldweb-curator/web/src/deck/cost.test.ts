import { describe, expect, it } from "vitest";
import type { PrintingOption } from "../card/prints";
import type { Card } from "../deck";
import type { PriceBook } from "../prices";
import {
  atCheapest,
  bands,
  breakdown,
  cheapest,
  DEFAULT_BANDS,
  loadBands,
  tierOf,
} from "./cost";

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
  it("splits on one copy's price, the proxy line inclusive", () => {
    expect(tierOf(10, DEFAULT_BANDS)).toBe("proxy");
    expect(tierOf(9.99, DEFAULT_BANDS)).toBe("buy");
    expect(tierOf(1, DEFAULT_BANDS)).toBe("buy");
    expect(tierOf(0.99, DEFAULT_BANDS)).toBe("ask");
  });

  it("refuses bands where asking around would start above proxying", () => {
    expect(bands(5, 2)).toBeNull();
    expect(bands(-1, 2)).toBeNull();
    expect(bands(Number.NaN, 2)).toBeNull();
    expect(bands(2, 2)).toEqual({ ask: 2, proxy: 2 });
  });

  it("remembers the bands, and falls back to the defaults on nonsense", () => {
    const stored = (v: string | null) => ({ getItem: () => v });
    expect(loadBands(stored('{"ask":0.5,"proxy":20}'))).toEqual({
      ask: 0.5,
      proxy: 20,
    });
    expect(loadBands(stored('{"ask":20,"proxy":0.5}'))).toEqual(DEFAULT_BANDS);
    expect(loadBands(stored("{"))).toEqual(DEFAULT_BANDS);
    expect(loadBands(stored(null))).toEqual(DEFAULT_BANDS);
  });

  it("tiers each line by a copy, dearest first, and totals every copy", () => {
    const b = breakdown(
      [
        card("Sol Ring"),
        // Four cheap copies are still cheap copies to ask around for.
        card("Lightning Bolt", { qty: 4 }),
        card("The One Ring", { finish: "foil" }),
        card("Jeweled Lotus"),
        card("Arcane Signet"),
      ],
      prices,
      "eur",
      DEFAULT_BANDS,
      nameOf,
    );
    expect(b.tiers.proxy.lines.map((l) => nameOf(l.card))).toEqual([
      "The One Ring",
      "Jeweled Lotus",
    ]);
    // A foil is priced as one.
    expect(b.tiers.proxy.total).toBe(100);
    expect(b.tiers.buy.lines.map((l) => nameOf(l.card))).toEqual(["Sol Ring"]);
    expect(b.tiers.ask.lines.map((l) => [nameOf(l.card), l.total])).toEqual([
      ["Lightning Bolt", 2],
      ["Arcane Signet", 0.4],
    ]);
    expect(b.tiers.ask.qty).toBe(5);
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
      DEFAULT_BANDS,
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
    const b = breakdown(
      [ring, signet, lotus],
      prices,
      "eur",
      DEFAULT_BANDS,
      nameOf,
    );
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
