import { beforeAll, describe, expect, it } from "vitest";
import type { DeckEntry } from "../github/decks";
import { commitEdit } from "../github/repoFile";
import { SETTINGS_PATH } from "../github/settings";
import { fileOf, loadWasm, watchedRepo } from "../github/testkit";
import { readSettings } from "../settings/rules";
import { order_decks } from "../wasm/pkg/meldweb_wasm.js";
import { familyOrder, moveDeck, moveInSettings, nudgeDeck } from "./variants";

const deck = (name: string, parent?: string): DeckEntry => ({
  stem: name.toLowerCase(),
  commit: "",
  sha: "",
  text: "",
  name,
  ...(parent ? { parent } : {}),
});

const names = (decks: DeckEntry[]) => familyOrder(decks).map((d) => d.name);

describe("the deck list", () => {
  it("puts each fork straight after its parent", () => {
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

  it("nests a fork of a fork, and lists an orphan by its name", () => {
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
  const p = (name: string) => name.toLowerCase();
  const stems = (order: string[] | null) => order;

  it("puts the declared decks first, in order, and the rest by name", () => {
    expect(
      familyOrder(decks, [p("Loam"), p("Lantern budget"), p("Lantern")]).map(
        (d) => d.name,
      ),
    ).toEqual(["Loam", "Lantern", "Lantern budget", "Lantern cEDH", "Atraxa"]);
  });

  it("moves a deck with its forks, and past a family whole", () => {
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

  it("keeps a fork among its siblings, and a deck out of another family", () => {
    const shown = familyOrder(decks);
    expect(
      stems(moveDeck(shown, p("Lantern cEDH"), p("Lantern budget"), "before")),
    ).toEqual(["atraxa", "lantern", "lantern cedh", "lantern budget", "loam"]);
    // A fork dropped outside its family has nowhere to go.
    expect(moveDeck(shown, p("Lantern cEDH"), p("Loam"), "before")).toBeNull();
    // A deck dropped on another's fork goes beside that family.
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
  const p = (name: string) => name.toLowerCase();

  it("steps over a whole family, and a fork only among its siblings", () => {
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

describe("a move saved to meldweb.toml", () => {
  beforeAll(loadWasm);
  const decks = ["A", "B", "C", "D"].map((n) => deck(n));
  const p = (name: string) => name.toLowerCase();
  const settings = (...names: string[]) =>
    order_decks(undefined, JSON.stringify(names.map(p)));
  const orderOf = (text: string | undefined) => {
    const read = readSettings(text ?? null);
    return read.kind === "read" ? read.decks : null;
  };

  it("is made again on the order another device saved meanwhile, keeping it", async () => {
    const mock = watchedRepo({ [SETTINGS_PATH]: settings("A", "B", "C", "D") });
    const { repo } = mock;
    const move = moveInSettings(decks, {
      kind: "drop",
      from: p("D"),
      to: p("A"),
      side: "before",
    });
    let first = true;
    await commitEdit(repo, SETTINGS_PATH, (t) => {
      if (first)
        void mock.elsewhere(SETTINGS_PATH, settings("C", "A", "B", "D"));
      first = false;
      return move(t);
    });
    expect(orderOf(await fileOf(repo, SETTINGS_PATH))).toEqual(
      ["C", "D", "A", "B"].map(p),
    );
  });

  it("writes nothing for a move that moves nothing on the order there, and refuses a file it cannot read", () => {
    const nudge = moveInSettings(decks, {
      kind: "nudge",
      stem: p("A"),
      by: -1,
    });
    expect(nudge(settings("A", "B", "C", "D"))).toBeNull();
    expect(() => nudge("decks = 3\n")).toThrow();
  });
});
