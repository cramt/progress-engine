import { beforeAll, describe, expect, it } from "vitest";
import { type OwnedCard, parseCollection } from "../collection";
import { loadWasm } from "../github/testkit";
import { sections } from "./sections";
import {
  BY_NAME,
  clickSort,
  loadView,
  ownedOrder,
  type Sort,
  saveView,
} from "./view";

beforeAll(loadWasm);

const TEXT = `cards = [
  { name = "Sol Ring", qty = 3 },
  { printing = "c21/263", finish = "foil" },
  { printing = "c21/9", qty = 2 },
  { name = "Island", qty = 40, finish = "etched" },
  { printing = "2xm/1", qty = 2 },
]
`;

const NAMES: Record<string, string> = {
  "c21/263": "Arcane Signet",
  "c21/9": "Alpha Tyrranax",
  "2xm/1": "Avacyn",
};

const nameOf = (c: OwnedCard) =>
  c.card.kind === "name"
    ? c.card.name
    : (NAMES[`${c.card.set}/${c.card.num}`] ?? "?");

/** Line totals; Island has no price. */
const PRICES: Record<string, number> = {
  "Sol Ring": 3,
  "Arcane Signet": 0.5,
  "Alpha Tyrranax": 0.25,
  Avacyn: 1.5,
};

function ordered(sort: Sort): string[] {
  const c = parseCollection(TEXT);
  if (c.kind !== "collection") throw new Error(c.message);
  const order = ownedOrder(sort, nameOf, (card) => PRICES[nameOf(card)]);
  return sections(c.places, c.cards, nameOf, "", order).flatMap((s) =>
    s.cards.map(nameOf),
  );
}

describe("ordering the collection", () => {
  it("is by name unless asked otherwise", () => {
    expect(ordered(BY_NAME)).toEqual([
      "Alpha Tyrranax",
      "Arcane Signet",
      "Avacyn",
      "Island",
      "Sol Ring",
    ]);
  });

  it("breaks a tie by name, whichever way round", () => {
    expect(ordered({ by: "qty", descending: true })).toEqual([
      "Island",
      "Sol Ring",
      "Alpha Tyrranax",
      "Avacyn",
      "Arcane Signet",
    ]);
    expect(ordered({ by: "qty", descending: false })).toEqual([
      "Arcane Signet",
      "Alpha Tyrranax",
      "Avacyn",
      "Sol Ring",
      "Island",
    ]);
  });

  it("puts a line with no price last, either way round", () => {
    expect(ordered({ by: "price", descending: true })).toEqual([
      "Sol Ring",
      "Avacyn",
      "Arcane Signet",
      "Alpha Tyrranax",
      "Island",
    ]);
    expect(ordered({ by: "price", descending: false })).toEqual([
      "Alpha Tyrranax",
      "Arcane Signet",
      "Avacyn",
      "Sol Ring",
      "Island",
    ]);
  });

  it("reads collector numbers as numbers, and an unpinned card goes last", () => {
    expect(ordered({ by: "printing", descending: false })).toEqual([
      "Avacyn",
      "Alpha Tyrranax",
      "Arcane Signet",
      "Island",
      "Sol Ring",
    ]);
  });

  it("orders finishes as the picker lists them", () => {
    expect(ordered({ by: "finish", descending: false })).toEqual([
      "Alpha Tyrranax",
      "Avacyn",
      "Sol Ring",
      "Arcane Signet",
      "Island",
    ]);
  });
});

describe("clicking a header", () => {
  it("flips the sort it already is", () => {
    expect(clickSort(BY_NAME, "name")).toEqual({
      by: "name",
      descending: true,
    });
  });

  it("starts the numbers biggest first and the rest from the top", () => {
    expect(clickSort(BY_NAME, "price").descending).toBe(true);
    expect(clickSort(BY_NAME, "qty").descending).toBe(true);
    expect(
      clickSort({ by: "price", descending: true }, "printing").descending,
    ).toBe(false);
  });
});

describe("the view kept in the browser", () => {
  const memory = () => {
    const items = new Map<string, string>();
    return {
      getItem: (k: string) => items.get(k) ?? null,
      setItem: (k: string, v: string) => void items.set(k, v),
    };
  };

  it("comes back as it was left, unsorted's fold included", () => {
    const storage = memory();
    saveView(
      {
        sort: { by: "price", descending: true },
        folded: new Set([null, "Bulk"]),
      },
      storage,
    );
    const view = loadView(storage);
    expect(view.sort).toEqual({ by: "price", descending: true });
    expect([...view.folded]).toEqual([null, "Bulk"]);
  });

  it("drops what it cannot read and keeps the rest", () => {
    const storage = memory();
    storage.setItem(
      "meldweb.collection-view",
      JSON.stringify({ sort: { by: "colour" }, folded: ["Bulk", 3] }),
    );
    const view = loadView(storage);
    expect(view.sort).toEqual(BY_NAME);
    expect([...view.folded]).toEqual(["Bulk"]);
    expect(loadView({ getItem: () => "{not json" }).sort).toEqual(BY_NAME);
  });
});
