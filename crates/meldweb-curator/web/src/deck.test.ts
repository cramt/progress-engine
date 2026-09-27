import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import {
  declareCategory,
  exportArchidekt,
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
    const imported = importArchidekt(
      "1x Rashmi and Ragavan (moc) 94 [Commander{top}]\n",
    );
    if (imported.kind !== "imported") throw new Error(imported.message);
    expect(imported.unreadable).toEqual([]);
    expect(imported.toml).toContain(
      '{ printing = "moc/94", in = ["Commander"] },  # Rashmi and Ragavan',
    );
    expect(parseDeck(imported.toml)).toMatchObject({ kind: "deck", total: 1 });
  });

  it("lists every line it could not read, and keeps the rest", () => {
    const imported = importArchidekt(
      "1x Sol Ring [Ramp]\nSol Ring without a count\n1x Arcane Signet\n",
    );
    if (imported.kind !== "imported") throw new Error(imported.message);
    expect(imported.unreadable).toEqual([
      {
        line: 2,
        text: "Sol Ring without a count",
        reason: expect.stringContaining("not a card"),
      },
    ]);
    expect(parseDeck(imported.toml)).toMatchObject({ kind: "deck", total: 2 });
  });

  it("refuses text with no card in it", () => {
    expect(importArchidekt("hello\n")).toMatchObject({ kind: "refused" });
  });
});

describe("copying as Archidekt", () => {
  it("writes lantern with names only, and it imports back to 100 cards", () => {
    const text = lantern();
    const names: Record<string, string> = {};
    for (const [, printing, name] of text.matchAll(
      /printing = "([^"]+)".*\},\s+# (.+)$/gm,
    )) {
      if (printing && name) names[printing] = name;
    }
    const archidekt = exportArchidekt(text, names);
    expect(archidekt).toContain("1x Rashmi and Ragavan [Commander{top}]\n");
    expect(archidekt).not.toMatch(/\(/);
    const back = importArchidekt(archidekt);
    if (back.kind !== "imported") throw new Error(back.message);
    expect(back.unreadable).toEqual([]);
    expect(parseDeck(back.toml)).toMatchObject({ kind: "deck", total: 100 });
  });

  it("throws, naming the printing, when a printing has no name", () => {
    expect(() => exportArchidekt(lantern())).toThrow(/moc\/346/);
  });
});
