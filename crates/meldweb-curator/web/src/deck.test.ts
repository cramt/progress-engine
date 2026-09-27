import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import {
  addCard,
  commitMessage,
  declareCategory,
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
    const withCard = addCard(text, { kind: "name", name: "Sol Ring" });
    const commanded = setCommander(withCard, 0);
    expect(deck(commanded).cards[0]?.place).toBe("commander");
    expect(deck(commanded).categories).toContainEqual({
      name: "Commander",
      kind: "commander",
    });
    expect(() => addCard(text, { kind: "name", name: "X" }, ["Nope"])).toThrow(
      /not declared/,
    );
    expect(() => setCardQty(text, 5, 2)).toThrow(/no card 5/);
  });

  it("adds a card as a new last line, or one more of it", () => {
    const text = newDeck("Test", "");
    expect(text).toBe('name = "Test"\n\ncards = [\n]\n');
    const once = addCard(text, {
      kind: "printing",
      set: "cmm",
      num: "410",
      name: "Sol Ring",
    });
    expect(once).toBe(
      'name = "Test"\n\ncards = [\n  { printing = "cmm/410" },  # Sol Ring\n]\n',
    );
    const twice = addCard(once, { kind: "printing", set: "cmm", num: "410" });
    expect(deck(twice).cards).toHaveLength(1);
    expect(deck(twice).cards[0]?.qty).toBe(2);
  });
});

describe("naming a deck", () => {
  it("names an imported deck at its top and renames one in place", () => {
    const imported = importArchidekt(
      "1x Rashmi and Ragavan (moc) 94 [Commander{top}]\n",
    );
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
    const after = addCard(removeCard(text, index), {
      kind: "name",
      name: "Sol Ring",
    });
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
    const text = importArchidekt(
      "1x Rashmi and Ragavan (moc) 94 [Commander{top}]\n",
    );
    expect(text).toContain(
      '{ printing = "moc/94", in = ["Commander"] },  # Rashmi and Ragavan',
    );
    expect(parseDeck(text)).toMatchObject({ kind: "deck", total: 1 });
  });
});
