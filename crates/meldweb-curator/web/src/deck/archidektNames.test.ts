import { describe, expect, it } from "vitest";
import type { Card, CardRef } from "../deck";
import type { Printings } from "../scryfall";
import { printingNames } from "./archidektNames";

const card = (index: number, ref: CardRef): Card => ({
  index,
  card: ref,
  qty: 1,
  finish: "nonfoil",
  categories: [],
  place: "in-deck",
  inDeck: true,
});

const known = (names: Record<string, string>): Printings =>
  new Map(
    Object.entries(names).map(([key, name]) => [
      key,
      { name, image: "", colorIdentity: [], typeLine: "" },
    ]),
  );

describe("the names the archidekt export needs", () => {
  it("name each printing as the file writes it, and nothing else", () => {
    const cards = [
      card(0, { kind: "printing", set: "PLST", num: "JOU-35" }),
      card(1, { kind: "name", name: "Sol Ring" }),
      card(2, { kind: "printing", set: "cmm", num: "1" }),
    ];
    expect(
      printingNames(
        cards,
        known({ "plst/jou-35": "Dakra Mystic", "name:sol ring": "Sol Ring" }),
      ),
    ).toEqual({ "PLST/JOU-35": "Dakra Mystic" });
  });
});
