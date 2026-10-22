import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadDeckSync } from "../deck";
import { moveSome, readSupply } from "../supply";
import { tradeText } from "../trade/TradeView";
import {
  offersOf,
  piles,
  printingLabel,
  progress,
  type ReadSupply,
  sleeved,
  sourcing,
  whereLabel,
} from "./sourcing";

beforeAll(() => {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
});

const PATH = "lantern";

const DECK = `cards = [
  { printing = "sld/2417" },  # Sol Ring
  { name = "Mind Stone" },
  { name = "Brainstorm" },
  { name = "The One Ring" },
  { name = "Island", qty = 3 },
]
`;

const MINE = `cards = [
  { printing = "cmm/400", at = "Lantern" },  # Sol Ring
  { name = "Mind Stone", at = "Bulk" },
  { name = "Brainstorm", at = "Loam" },
  { name = "Counterspell", at = "Lantern" },
  { name = "Island" },
]

[places]
Bulk = {}
Lantern = { deck = "lantern" }
Loam = { deck = "loam" }
`;

const ALICE = `cards = [
  { printing = "ltr/451", finish = "foil", at = "Binder" },  # The One Ring
]

[places]
Binder = {}
`;

function read(mine = MINE): ReadSupply {
  const s = readSupply(PATH, DECK, mine, [{ who: "alice", text: ALICE }]);
  if (s.kind !== "supply") throw new Error(s.message);
  return s;
}

describe("sourcing a deck", () => {
  it("sorts each copy by where it comes from", () => {
    const s = sourcing(read());
    const names = (rows: { line: { name: string } }[]) =>
      rows.map((r) => r.line.name);
    expect(names(s.have)).toEqual(["Brainstorm", "Island", "Mind Stone"]);
    expect(names(s.alike)).toEqual(["Sol Ring"]);
    expect(s.friends.map((c) => c.name)).toEqual(["The One Ring"]);
    expect(s.buy).toEqual([]);
    expect(s.basics.map((c) => [c.name, c.short])).toEqual([["Island", 2]]);
    expect(s.copies).toEqual({
      need: 7,
      have: 3,
      alike: 1,
      friends: 1,
      buy: 0,
      basics: 2,
    });
  });

  it("writes a friend the list of what they hold, by their place", () => {
    const text = tradeText("me", "alice", offersOf(read(), "alice"), "Lantern");
    expect(text).toBe(
      "Cards of alice's that me wants for Lantern:\n\nBinder\n1 The One Ring (LTR 451, foil)\n",
    );
  });

  it("names a copy by printing and finish, and a place as the holder would", () => {
    expect(
      printingLabel({ kind: "printing", set: "cmm", num: "400" }, "foil"),
    ).toBe("CMM 400 · foil");
    expect(printingLabel({ kind: "name", name: "Sol Ring" }, "nonfoil")).toBe(
      "",
    );
    expect(whereLabel({ whereabouts: "loose" })).toBe("Unsorted");
    expect(whereLabel({ at: "Loam", whereabouts: "otherDeck" })).toBe(
      "In Loam",
    );
  });
});

describe("building a deck", () => {
  it("pulls from loose places before another deck, and counts what is left", () => {
    const s = read();
    expect(
      piles(s).map((p) => [p.at ?? null, p.rows.map((r) => r.line.name)]),
    ).toEqual([
      ["Bulk", ["Mind Stone"]],
      [null, ["Island"]],
      ["Loam", ["Brainstorm"]],
    ]);
    expect(sleeved(s).map((r) => r.line.name)).toEqual(["Sol Ring"]);
    expect(s.extra.map((e) => [e.name, e.listed])).toEqual([
      ["Counterspell", false],
    ]);
    expect(progress(s)).toEqual({
      need: 7,
      inDeck: 1,
      standIns: 1,
      toPull: 3,
      toTakeOut: 1,
      missing: 3,
    });
  });

  it("moves a pile into the deck as one edit, and the deck then holds it", () => {
    const s = read();
    const bulk = piles(s)[0];
    if (!bulk) throw new Error("no pile");
    const moved = moveSome(
      MINE,
      bulk.rows.map((r) => ({ line: r.copies.line, qty: r.copies.qty })),
      "Lantern",
    );
    expect(sleeved(read(moved)).map((r) => r.line.name)).toEqual([
      "Mind Stone",
      "Sol Ring",
    ]);
  });
});
