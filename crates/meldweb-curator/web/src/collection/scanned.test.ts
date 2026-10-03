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

  it("joins the line that already holds it in that place", () => {
    const after = addScanned(TEXT, lotus);
    expect(after).toContain(`{ printing = "lea/232", qty = 2, at = "Binder" }`);
  });

  it("is a new line in another place, or in another finish", () => {
    const unsorted = addScanned(TEXT, { ...lotus, at: null });
    expect(cards(unsorted)).toHaveLength(3);
    const foil = addScanned(TEXT, { ...lotus, finish: "foil" });
    expect(cards(foil).find((c) => c.finish === "foil")).toMatchObject({
      qty: 1,
      at: "Binder",
    });
  });

  it("goes in by name when the printing is not kept", () => {
    const after = addScanned(TEXT, {
      card: { kind: "name", name: "Sol Ring" },
      at: null,
      finish: "nonfoil",
    });
    expect(after).toContain(`{ name = "Sol Ring", qty = 4 }`);
  });

  it("is taken back off the line it went on", () => {
    const after = addScanned(TEXT, lotus);
    expect(removeScanned(after, lotus)).toBe(TEXT);
  });

  it("taken back as the last copy removes the line", () => {
    const after = removeScanned(TEXT, lotus);
    expect(after).not.toContain("lea/232");
  });

  it("is refused taking back once it has moved", () => {
    expect(() => removeScanned(TEXT, { ...lotus, at: null })).toThrow(
      /no longer where the scan put it/,
    );
  });
});
