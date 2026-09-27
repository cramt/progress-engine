import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import {
  declareCategory,
  importArchidekt,
  loadDeckSync,
  parseDeck,
  setCardCategories,
} from "./deck";

const repo = new URL("../../../../", import.meta.url);
const lantern = () =>
  readFileSync(new URL("decks/lantern.deck.toml", repo), "utf8");

beforeAll(() => {
  loadDeckSync(
    readFileSync(
      new URL("../src/wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url),
    ),
  );
});

describe("the deck, through wasm", () => {
  it("reads lantern.deck.toml as Gauntlet does: 100 cards, Rashmi commanding", () => {
    const parsed = parseDeck(lantern());
    if (parsed.kind !== "deck") throw new Error(parsed.message);
    expect(parsed.total).toBe(100);
    expect(parsed.cards.filter((c) => c.place === "commander")).toHaveLength(1);
  });

  it("refuses an edit that puts a card in two places", () => {
    const text = lantern();
    const parsed = parseDeck(text);
    if (parsed.kind !== "deck") throw new Error(parsed.message);
    const rashmi = parsed.cards.find((c) => c.place === "commander");
    if (!rashmi) throw new Error("no commander");
    const withSide = declareCategory(text, "Side", "sideboard");
    expect(() =>
      setCardCategories(withSide, rashmi.index, [...rashmi.categories, "Side"]),
    ).toThrow(/two places/);
  });

  it("says why a deck file is refused", () => {
    expect(parseDeck("cards = [{ qty = 1 }]")).toMatchObject({
      kind: "refused",
    });
  });
});

describe("importing Archidekt", () => {
  it("types a commander category and names the printing in a comment", () => {
    const text = importArchidekt(
      "1x Rashmi and Ragavan (moc) 94 [Commander{top}]\n",
    );
    expect(text).toContain(
      '{ printing = "moc/94", in = ["Commander"] },  # Rashmi and Ragavan',
    );
    expect(parseDeck(text)).toMatchObject({ kind: "deck", total: 1 });
  });
});
