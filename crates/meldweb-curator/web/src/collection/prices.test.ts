import { describe, expect, it } from "vitest";
import type { OwnedCard } from "../collection";
import { loadCurrency, type PriceBook, unitPrice, worth } from "./prices";

const owned = (
  card: OwnedCard["card"],
  qty: number,
  finish: OwnedCard["finish"] = "nonfoil",
): OwnedCard => ({ index: 0, card, qty, finish });

const ring = { kind: "name", name: "Sol Ring" } as const;
const ragavan = { kind: "printing", set: "moc", num: "94" } as const;

const prices: PriceBook = new Map([
  ["name:sol ring", { eur: { nonfoil: 1.25 }, usd: { nonfoil: 1.5 } }],
  ["moc/94", { eur: { nonfoil: 2, foil: 5 }, usd: {} }],
]);

describe("a collection's prices", () => {
  it("price each copy in its own finish", () => {
    expect(unitPrice(owned(ragavan, 1, "foil"), prices, "eur")).toBe(5);
    expect(unitPrice(owned(ragavan, 1), prices, "eur")).toBe(2);
    // No etched price is no price, not the nonfoil one.
    expect(unitPrice(owned(ragavan, 1, "etched"), prices, "eur")).toBe(
      undefined,
    );
  });

  it("total every copy, counting the ones with no price apart", () => {
    const cards = [
      owned(ring, 3),
      owned(ragavan, 2, "foil"),
      owned({ kind: "name", name: "Unknown" }, 4),
    ];
    expect(worth(cards, prices, "eur")).toEqual({ total: 13.75, unpriced: 4 });
    expect(worth(cards, prices, "usd")).toEqual({ total: 4.5, unpriced: 6 });
  });

  it("remember the currency, and fall back to euros", () => {
    expect(loadCurrency({ getItem: () => "usd" })).toBe("usd");
    expect(loadCurrency({ getItem: () => "gbp" })).toBe("eur");
    expect(
      loadCurrency({
        getItem: () => {
          throw new Error("blocked");
        },
      }),
    ).toBe("eur");
  });
});
