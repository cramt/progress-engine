import { describe, expect, it } from "vitest";
import type { Card, CardRef } from "../deck";
import type { Printings } from "../scryfall";
import { composeQuery, deckIdentity, filterText } from "./query";

const card = (
  index: number,
  ref: CardRef,
  place: Card["place"] = "in-deck",
): Card => ({
  index,
  card: ref,
  qty: 1,
  finish: "nonfoil",
  categories: [],
  place,
  inDeck: true,
});

const printings = (entries: [string, string[]][]): Printings =>
  new Map(
    entries.map(([key, colorIdentity]) => [
      key,
      {
        id: key,
        name: key,
        set: "",
        num: "",
        image: "",
        colorIdentity,
        typeLine: "",
      },
    ]),
  );

describe("the deck's identity", () => {
  it("joins its commanders' identities in WUBRG order", () => {
    const cards = [
      card(0, { kind: "printing", set: "moc", num: "94" }, "commander"),
      card(1, { kind: "name", name: "Tymna the Weaver" }, "commander"),
      card(2, { kind: "name", name: "Sol Ring" }),
    ];
    const known = printings([
      ["moc/94", ["G", "R", "U"]],
      ["name:tymna the weaver", ["B", "W"]],
      ["name:sol ring", []],
    ]);
    expect(deckIdentity(cards, known)).toBe("WUBRG");
  });

  it("is C for a colourless commander", () => {
    const cards = [card(0, { kind: "name", name: "Karn" }, "commander")];
    expect(deckIdentity(cards, printings([["name:karn", []]]))).toBe("C");
  });

  it("is unknown without a commander, or with one Scryfall has not described", () => {
    const sol = card(0, { kind: "name", name: "Sol Ring" });
    expect(deckIdentity([sol], printings([["name:sol ring", []]]))).toBeNull();
    const cmdr = card(1, { kind: "name", name: "Missing" }, "commander");
    expect(deckIdentity([cmdr], printings([]))).toBeNull();
  });
});

describe("smart filters", () => {
  it("wrap the user's query so an `or` stays inside it", () => {
    expect(
      composeQuery("o:draw or o:scry", { identity: "WG", format: "commander" }),
    ).toBe("(o:draw or o:scry) id<=WG f:commander");
  });

  it("leave out what the deck does not say", () => {
    expect(composeQuery("t:artifact", { identity: "URG", format: null })).toBe(
      "(t:artifact) id<=URG",
    );
    expect(composeQuery("t:artifact", { identity: null, format: null })).toBe(
      "t:artifact",
    );
    expect(filterText({ identity: null, format: "modern" })).toBe("f:modern");
  });

  it("send the query as typed when off, and nothing for an empty box", () => {
    expect(composeQuery("  t:artifact mv<=2 ", null)).toBe("t:artifact mv<=2");
    expect(composeQuery("   ", { identity: "U", format: "commander" })).toBe(
      "",
    );
  });
});
