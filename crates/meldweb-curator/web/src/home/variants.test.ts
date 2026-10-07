import { describe, expect, it } from "vitest";
import type { DeckEntry } from "../github/decks";
import { familyOrder, moveDeck, nudgeDeck } from "./variants";

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

describe("ordering the deck list", () => {
  const decks = [
    deck("Atraxa"),
    deck("Lantern"),
    deck("Lantern cEDH", "lantern"),
    deck("Lantern budget", "lantern"),
    deck("Loam"),
  ];
  const p = (name: string) => `decks/${name.toLowerCase()}.deck.toml`;
  const stems = (paths: string[] | null) =>
    paths?.map((x) => x.slice("decks/".length, -".deck.toml".length));

  it("puts the declared decks first, in order, and the rest by name", () => {
    expect(
      familyOrder(decks, [p("Loam"), p("Lantern budget"), p("Lantern")]).map(
        (d) => d.name,
      ),
    ).toEqual(["Loam", "Lantern", "Lantern budget", "Lantern cEDH", "Atraxa"]);
  });

  it("moves a deck with its variants, and past a family whole", () => {
    const shown = familyOrder(decks);
    expect(stems(moveDeck(shown, p("Lantern"), p("Atraxa"), "before"))).toEqual(
      ["lantern", "lantern budget", "lantern cedh", "atraxa", "loam"],
    );
    expect(stems(moveDeck(shown, p("Atraxa"), p("Lantern"), "after"))).toEqual([
      "lantern",
      "lantern budget",
      "lantern cedh",
      "atraxa",
      "loam",
    ]);
  });

  it("keeps a variant among its siblings, and a deck out of another family", () => {
    const shown = familyOrder(decks);
    expect(
      stems(moveDeck(shown, p("Lantern cEDH"), p("Lantern budget"), "before")),
    ).toEqual(["atraxa", "lantern", "lantern cedh", "lantern budget", "loam"]);
    // A variant dropped outside its family has nowhere to go.
    expect(moveDeck(shown, p("Lantern cEDH"), p("Loam"), "before")).toBeNull();
    // A deck dropped on another's variant goes beside that family.
    expect(
      stems(moveDeck(shown, p("Loam"), p("Lantern budget"), "before")),
    ).toEqual(["atraxa", "loam", "lantern", "lantern budget", "lantern cedh"]);
    expect(
      moveDeck(shown, p("Lantern"), p("Lantern cEDH"), "after"),
    ).toBeNull();
    expect(moveDeck(shown, p("Atraxa"), p("Lantern"), "before")).toBeNull();
  });
});

describe("nudging a deck", () => {
  const decks = [
    deck("Atraxa"),
    deck("Lantern"),
    deck("Lantern cEDH", "lantern"),
    deck("Lantern budget", "lantern"),
    deck("Loam"),
  ];
  const p = (name: string) => `decks/${name.toLowerCase()}.deck.toml`;

  it("steps over a whole family, and a variant only among its siblings", () => {
    const shown = familyOrder(decks);
    expect(nudgeDeck(shown, p("Loam"), -1)).toEqual(
      ["Atraxa", "Loam", "Lantern", "Lantern budget", "Lantern cEDH"].map(p),
    );
    expect(nudgeDeck(shown, p("Atraxa"), -1)).toBeNull();
    expect(nudgeDeck(shown, p("Lantern cEDH"), 1)).toBeNull();
    expect(nudgeDeck(shown, p("Lantern cEDH"), -1)).toEqual(
      ["Atraxa", "Lantern", "Lantern cEDH", "Lantern budget", "Loam"].map(p),
    );
  });
});
