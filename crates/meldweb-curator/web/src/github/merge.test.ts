import { beforeAll, describe, expect, it } from "vitest";
import { mergeFile } from "./merge";
import { loadWasm } from "./testkit";

// The merge is the wasm's.
beforeAll(loadWasm);

const lines = (...l: string[]) => `${l.join("\n")}\n`;

describe("merging a deck two clients edited, card by card", () => {
  const deck = (...cards: string[]) =>
    lines('name = "Lantern"', "cards = [", ...cards.map((c) => `  ${c},`), "]");
  const BASE = deck(
    '{ name = "Sol Ring" }',
    '{ name = "Mind Stone" }',
    '{ name = "Island", qty = 10 }',
  );

  it("keeps both when they changed neighbouring cards", () => {
    const ours = BASE.replace('"Sol Ring" }', '"Sol Ring", qty = 2 }');
    const theirs = BASE.replace('"Mind Stone" }', '"Mind Stone", qty = 3 }');
    expect(mergeFile("lantern.deck.toml", BASE, ours, theirs)).toEqual({
      kind: "merged",
      text: deck(
        '{ name = "Sol Ring", qty = 2 }',
        '{ name = "Mind Stone", qty = 3 }',
        '{ name = "Island", qty = 10 }',
      ),
    });
  });

  it("lands their quantity on our pinned printing", () => {
    const ours = BASE.replace(
      '{ name = "Sol Ring" },',
      '{ printing = "sld/2417" },  # Sol Ring',
    );
    const theirs = BASE.replace('"Sol Ring" }', '"Sol Ring", qty = 2 }');
    const merged = mergeFile("lantern.deck.toml", BASE, ours, theirs);
    expect(merged).toEqual({
      kind: "merged",
      text: ours.replace('"sld/2417" }', '"sld/2417", qty = 2 }'),
    });
  });

  it("names the card and what both changed when they clash", () => {
    const ours = BASE.replace("qty = 10", "qty = 9");
    const theirs = BASE.replace("qty = 10", "qty = 11");
    expect(mergeFile("lantern.deck.toml", BASE, ours, theirs)).toEqual({
      kind: "clashed",
      clashes: [
        {
          path: "lantern.deck.toml",
          index: 0,
          subject: "Island",
          attribute: "quantity",
          ours: "9",
          theirs: "11",
        },
      ],
    });
  });
});

describe("merging any other file by lines", () => {
  const BASE = lines("cards = [", "  a,", "  b,", "  c,", "  d,", "]");
  const merge = (ours: string, theirs: string) =>
    mergeFile("notes.txt", BASE, ours, theirs);

  it("keeps both when they changed different lines", () => {
    const ours = lines("cards = [", "  A,", "  b,", "  c,", "  d,", "]");
    const theirs = lines("cards = [", "  a,", "  b,", "  c,", "  D,", "]");
    expect(merge(ours, theirs)).toEqual({
      kind: "merged",
      text: lines("cards = [", "  A,", "  b,", "  c,", "  D,", "]"),
    });
  });

  it("takes the one side that changed, and an edit both made alike", () => {
    const changed = lines("cards = [", "  a,", "  B,", "  c,", "  d,", "]");
    for (const [ours, theirs] of [
      [BASE, changed],
      [changed, BASE],
      [changed, changed],
    ] as const) {
      expect(merge(ours, theirs)).toEqual({ kind: "merged", text: changed });
    }
  });

  it("gives up when both changed the same line differently", () => {
    const ours = lines("cards = [", "  a,", "  B1,", "  c,", "  d,", "]");
    const theirs = lines("cards = [", "  a,", "  B2,", "  c,", "  d,", "]");
    expect(merge(ours, theirs)).toEqual({ kind: "clashed", clashes: [] });
  });
});
