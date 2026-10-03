import { describe, expect, it } from "vitest";
import type { DeckEntry } from "../github/decks";
import { familyOrder } from "./variants";

const deck = (name: string, variantOf?: string): DeckEntry => ({
  path: `decks/${name.toLowerCase()}.deck.toml`,
  sha: "",
  name,
  ...(variantOf ? { variantOf: `decks/${variantOf}.deck.toml` } : {}),
});

const names = (decks: DeckEntry[]) => familyOrder(decks).map((d) => d.name);

describe("the deck list", () => {
  it("puts each variant straight after its parent", () => {
    expect(
      names([
        deck("Loam"),
        deck("Zur budget", "lantern"),
        deck("Lantern"),
        deck("Lantern cEDH", "lantern"),
        deck("Atraxa"),
      ]),
    ).toEqual(["Atraxa", "Lantern", "Lantern cEDH", "Zur budget", "Loam"]);
  });

  it("nests a variant of a variant, and lists an orphan by its name", () => {
    expect(
      names([
        deck("Lantern"),
        deck("Mini", "budget"),
        deck("Budget", "lantern"),
        deck("Orphan", "gone"),
      ]),
    ).toEqual(["Lantern", "Budget", "Mini", "Orphan"]);
  });

  it("places every deck once, even in a cycle", () => {
    expect(names([deck("A", "b"), deck("B", "a")]).sort()).toEqual(["A", "B"]);
  });
});
