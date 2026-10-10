import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import {
  commitMessage,
  deckAdd,
  declareCategory,
  exportArchidekt,
  exportDeck,
  importArchidekt,
  loadDeckSync,
  newDeck,
  parseDeck,
  removeCard,
  setCardCategories,
  setCardFinish,
  setCardPrinting,
  setCardQty,
  setCommander,
  setDeckMeta,
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

/** The lines of `after` that differ from `before`'s, both the same length. */
function changedLines(before: string, after: string): [string, string][] {
  const a = before.split("\n");
  const b = after.split("\n");
  expect(b).toHaveLength(a.length);
  return a.flatMap((line, i): [string, string][] =>
    line === b[i] ? [] : [[line, b[i] ?? ""]],
  );
}

function deck(text: string) {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") throw new Error(parsed.message);
  return parsed;
}

describe("editing a card, through wasm", () => {
  const sculpt = (text: string) => {
    const card = deck(text).cards.find((c) =>
      c.categories.includes("Lantern - Sculpt"),
    );
    if (!card) throw new Error("no sculpt card");
    return card;
  };

  it("sets a quantity on one line, and 0 removes that line", () => {
    const text = lantern();
    const { index } = sculpt(text);
    const three = setCardQty(text, index, 3);
    const changed = changedLines(text, three);
    expect(changed).toHaveLength(1);
    expect(changed[0]?.[1]).toMatch(/qty = 3/);
    expect(deck(three).cards[index]?.qty).toBe(3);
    const gone = setCardQty(text, index, 0);
    expect(gone).toBe(removeCard(text, index));
    expect(deck(gone).total).toBe(99);
    expect(gone.split("\n")).toHaveLength(text.split("\n").length - 1);
  });

  it("changes a finish and a printing on one line, keeping the name comment", () => {
    const text = lantern();
    const { index } = sculpt(text);
    const foil = setCardFinish(text, index, "foil");
    expect(changedLines(text, foil)[0]?.[1]).toMatch(/finish = "foil"/);
    expect(setCardFinish(foil, index, "nonfoil")).toBe(text);
    const printed = setCardPrinting(text, index, "SLD", "1");
    const changed = changedLines(text, printed);
    expect(changed).toHaveLength(1);
    const [before = "", after = ""] = changed[0] ?? [];
    expect(after).toMatch(/printing = "sld\/1"/);
    expect(after.split("#")[1]).toBe(before.split("#")[1]);
  });

  it("makes a card a commander and refuses what the format refuses", () => {
    const text = newDeck("Test", "commander");
    const withCard = deckAdd(text, { kind: "name", name: "Sol Ring" }).text;
    const commanded = setCommander(withCard, 0);
    expect(deck(commanded).cards[0]?.place).toBe("commander");
    expect(deck(commanded).categories).toContainEqual({
      name: "Commander",
      kind: "commander",
    });
    expect(() =>
      deckAdd(
        text,
        { kind: "name", name: "X" },
        { kind: "categories", categories: ["Nope"] },
      ),
    ).toThrow(/not declared/);
    expect(() => setCardQty(text, 5, 2)).toThrow(/no card 5/);
  });

  it("adds a card as a new last line, or one more of it, saying which", () => {
    const text = newDeck("Test", "");
    expect(text).toBe('name = "Test"\n\ncards = [\n]\n');
    const once = deckAdd(text, {
      kind: "printing",
      set: "cmm",
      num: "410",
      name: "Sol Ring",
    });
    expect(once).toEqual({
      text: 'name = "Test"\n\ncards = [\n  { printing = "cmm/410" },  # Sol Ring\n]\n',
      line: 0,
      made: true,
    });
    const twice = deckAdd(once.text, {
      kind: "printing",
      set: "cmm",
      num: "410",
    });
    expect(twice).toMatchObject({ line: 0, made: false });
    expect(deck(twice.text).cards[0]?.qty).toBe(2);
  });

  it("finds a card's line by the names it is handed for printings", () => {
    const text = `cards = [\n  { printing = "isd/51" },\n]\n`;
    const names = { "isd/51": "Delver of Secrets // Insectile Aberration" };
    const card = { kind: "name", name: "Delver of Secrets" } as const;
    expect(deckAdd(text, card, { kind: "automatic" }, names)).toMatchObject({
      line: 0,
      made: false,
    });
    expect(deckAdd(text, card).made).toBe(true);
  });
});

describe("naming a deck", () => {
  it("names an imported deck at its top and renames one in place", () => {
    const result = importArchidekt(
      "1x Rashmi and Ragavan (moc) 94 [Commander{top}]\n",
    );
    if (result.kind !== "imported") throw new Error(JSON.stringify(result));
    const imported = result.toml;
    const named = setDeckMeta(imported, "Lantern", "commander");
    expect(named).toBe(`name = "Lantern"\nformat = "commander"\n\n${imported}`);
    expect(deck(named)).toMatchObject({
      name: "Lantern",
      format: "commander",
    });
    const renamed = setDeckMeta(named, "Lantern Control");
    expect(changedLines(named, renamed)).toEqual([
      ['name = "Lantern"', 'name = "Lantern Control"'],
    ]);
  });
});

describe("the commit message", () => {
  it("reads as the deck's changelog", () => {
    const text = lantern();
    const { index } = deck(text).cards[0] ?? { index: -1 };
    // In no category: lantern's own Sol Ring is in one, so this is a line.
    const after = deckAdd(
      removeCard(text, index),
      { kind: "name", name: "Sol Ring" },
      { kind: "categories", categories: [] },
    ).text;
    expect(commitMessage(text, after, "decks/lantern.deck.toml")).toBe(
      "lantern: +1 Sol Ring, -1 Academy Manufactor",
    );
    expect(commitMessage(text, text, "decks/lantern.deck.toml")).toBe(
      "lantern: reformat",
    );
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

describe("copying for another tool", () => {
  const named = () => {
    const text = lantern();
    const names: Record<string, string> = {};
    for (const [, printing, name] of text.matchAll(
      /printing = "([^"]+)".*\},\s+# (.+)$/gm,
    )) {
      if (printing && name) names[printing] = name;
    }
    return { text, names };
  };

  it("gives Cockatrice all 100 cards, the commander as a sideboard line", () => {
    const { text, names } = named();
    const out = exportDeck(text, "cockatrice", names);
    expect(out).toMatch(/^SB: 1 Rashmi and Ragavan \(MOC\) 94$/m);
    const count = (lines: string[]) =>
      lines.reduce(
        (n, l) => n + Number(l.replace(/^SB: /, "").split(" ")[0]),
        0,
      );
    expect(count(out.split("\n").filter((l) => l !== ""))).toBe(100);
  });

  it("gives Cardmarket one line a card, names only", () => {
    const { text, names } = named();
    const out = exportDeck(text, "cardmarket", names);
    const lines = out.trimEnd().split("\n");
    expect(lines.every((l) => /^\d+ [^()]+$/.test(l))).toBe(true);
    expect(new Set(lines.map((l) => l.replace(/^\d+ /, ""))).size).toBe(
      lines.length,
    );
  });

  it("gives Tabletop Simulator all 100 cards, the commander under its own heading", () => {
    const { text, names } = named();
    const out = exportDeck(text, "tabletop-simulator", names);
    expect(out).toMatch(
      /^Commander\n1 Rashmi and Ragavan \(MOC\) 94\n\nDeck\n/,
    );
    const count = out
      .split("\n")
      .filter((l) => /^\d/.test(l))
      .reduce((n, l) => n + Number(l.split(" ")[0]), 0);
    expect(count).toBe(100);
  });

  it("throws, naming the printing, for every tool", () => {
    for (const to of [
      "cockatrice",
      "cardmarket",
      "tabletop-simulator",
    ] as const)
      expect(() => exportDeck(lantern(), to)).toThrow(/moc\/346/);
  });
});
