import { beforeAll, describe, expect, it } from "vitest";
import {
  importCollection,
  parseCollection,
  readCollectionExport,
} from "../collection";
import { loadWasm } from "../github/testkit";

beforeAll(loadWasm);

// What a row means and which printing it pins are chip-decklist's, tested
// there (tests/collection_read.rs, tests/collection_import.rs); these are the
// page's two calls over it.

const MOXFIELD = `"Count","Tradelist Count","Name","Edition","Condition","Language","Foil","Tags","Last Modified","Collector Number","Alter","Proxy","Purchase Price"
"2","0","Sol Ring","cmm","Near Mint","English","foil","","2025-01-02 10:00:00.000000","400","False","False",""
"1","0","Llanowar Elves","xxx","Near Mint","English","","","2025-01-02 10:00:00.000000","1","False","False",""
`;

describe("a collection import", () => {
  it("says whose export it is and what Scryfall must be asked", () => {
    const read = readCollectionExport(MOXFIELD);
    expect(read).toMatchObject({
      kind: "export",
      source: "Moxfield",
      rows: 2,
      copies: 3,
      places: [],
      unplaced: true,
      dropped: [{ column: "Condition", rows: 2 }],
    });
    if (read.kind !== "export") return;
    expect(read.asks).toEqual([
      { kind: "printing", set: "cmm", num: "400" },
      { kind: "printing", set: "xxx", num: "1" },
    ]);
  });

  it("goes in as one edit, by name where Scryfall had no answer", () => {
    const imported = importCollection(
      "",
      MOXFIELD,
      [{ id: "a", set: "cmm", num: "400", name: "Sol Ring" }],
      false,
      "Binder",
    );
    const parsed = parseCollection(imported.text);
    expect(parsed).toMatchObject({
      kind: "collection",
      places: [{ name: "Binder" }],
      total: 3,
    });
    if (parsed.kind !== "collection") return;
    expect(parsed.cards.map((c) => [c.card, c.finish])).toEqual([
      [{ kind: "printing", set: "cmm", num: "400" }, "foil"],
      [{ kind: "name", name: "Llanowar Elves" }, "nonfoil"],
    ]);
    expect(imported.notes).toEqual([
      { line: 3, reason: "Scryfall has no xxx/1; kept by name" },
    ]);
  });

  it("refuses text with no card in it", () => {
    expect(readCollectionExport("Name,Quantity\n")).toMatchObject({
      kind: "refused",
    });
  });
});
