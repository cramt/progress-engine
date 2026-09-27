import { describe, expect, it } from "vitest";
import type { Card, CardRef } from "../deck";
import type { Printings } from "../scryfall";
import { missingCards, printingNames } from "./printings";

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
      { name, image: "", colorIdentity: [] },
    ]),
  );

describe("the printings an edit leaves unfetched", () => {
  const deck = [
    card(0, { kind: "printing", set: "cmr", num: "472" }),
    card(1, { kind: "name", name: "Sol Ring" }),
    card(2, { kind: "name", name: "Brainstorm" }),
    card(3, { kind: "printing", set: "PLST", num: "JOU-35" }),
    card(4, { kind: "name", name: "brainstorm" }),
  ];

  it("are the cards neither found nor asked for, once each", () => {
    const missing = missingCards(
      deck,
      known({ "cmr/472": "Sol Ring", "name:sol ring": "Sol Ring" }),
      new Set(),
    );
    expect(missing.map((c) => c.index)).toEqual([2, 3]);
  });

  it("leave out what was already asked for, found or not", () => {
    const missing = missingCards(
      deck,
      known({}),
      new Set(["cmr/472", "name:sol ring", "name:brainstorm", "plst/jou-35"]),
    );
    expect(missing).toEqual([]);
  });
});

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
