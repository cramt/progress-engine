import { beforeAll, describe, expect, it } from "vitest";
import { parseCollection } from "../collection";
import { loadWasm } from "../github/testkit";
import {
  addScanned,
  moveScanned,
  removeScanned,
  type ScannedCopy,
} from "./scanned";

beforeAll(loadWasm);

// Which line a copy joins and comes off is chip-decklist's, tested there
// (tests/add.rs); these are the scanner's own steps over it.

const TEXT = `cards = [
  { printing = "lea/232", at = "Binder" },  # Black Lotus
  { name = "Sol Ring", qty = 3 },
]

[places]
Binder = {}
`;

const lotus: ScannedCopy = {
  card: { kind: "printing", set: "lea", num: "232", name: "Black Lotus" },
  at: "Binder",
  finish: "nonfoil",
};

function cards(text: string) {
  const c = parseCollection(text);
  if (c.kind !== "collection") throw new Error(c.message);
  return c.cards;
}

describe("a scanned copy", () => {
  it("moves to another place, leaving the rest of its line behind", () => {
    const two = addScanned(TEXT, lotus);
    const moved = cards(moveScanned(two, lotus, null));
    expect(moved.find((c) => c.at === "Binder")).toMatchObject({ qty: 1 });
    expect(
      moved.find((c) => c.at === undefined && c.card.kind === "printing"),
    ).toMatchObject({ qty: 1 });
  });

  it("refuses to move a copy that is no longer where the scan put it", () => {
    expect(() =>
      moveScanned(TEXT, { ...lotus, finish: "foil" }, null),
    ).toThrow();
  });

  it("is taken back off the line it went on", () => {
    expect(removeScanned(addScanned(TEXT, lotus), lotus)).toBe(TEXT);
    expect(() => removeScanned(TEXT, { ...lotus, at: null })).toThrow(
      /no line holds lea\/232 unsorted/,
    );
  });
});
