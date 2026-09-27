import { describe, expect, it } from "vitest";
import type { Entry } from "../decklist";
import { groupByCategory, packColumns } from "./layout";

let line = 0;
const entry = (name: string, category: string, commander = false): Entry => ({
  line: ++line,
  qty: 1,
  name,
  foil: false,
  categories: [{ name: category, flags: commander ? ["top"] : [] }],
  commander,
  outside: false,
});

describe("grouping", () => {
  it("puts the commander's category first and the rest alphabetically", () => {
    const groups = groupByCategory([
      entry("Island", "Land"),
      entry("Sol Ring", "Artifact"),
      entry("Rashmi and Ragavan", "Commander", true),
    ]);
    expect(groups.map((g) => g.name)).toEqual([
      "Commander",
      "Artifact",
      "Land",
    ]);
  });
});

describe("packing", () => {
  it("fills the first row in order, then the shortest column", () => {
    const columns = packColumns(
      ["a", "b", "c", "d"],
      (x) => (x === "a" ? 1 : 5),
      3,
    );
    expect(columns).toEqual([["a", "d"], ["b"], ["c"]]);
  });
});
