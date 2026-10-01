import { beforeAll, describe, expect, it } from "vitest";
import { type OwnedCard, parseCollection } from "../collection";
import { loadWasm } from "../github/testkit";
import type { Printings } from "../scryfall";
import { addOwnedByName, sections } from "./sections";

beforeAll(loadWasm);

const TEXT = `cards = [
  { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan
  { name = "Sol Ring", qty = 3, at = "Bulk" },
  { name = "Island", qty = 40 },
  { name = "Arcane Signet", at = "Bulk" },
]

[places]
Bulk = {}
"Trade binder" = {}
Lantern = { deck = "decks/lantern.deck.toml" }
`;

const printings: Printings = new Map([
  [
    "moc/94",
    {
      name: "Rashmi and Ragavan",
      set: "moc",
      num: "94",
      image: "",
      colorIdentity: [],
      typeLine: "",
    },
  ],
]);

function parsed() {
  const c = parseCollection(TEXT);
  if (c.kind !== "collection") throw new Error(c.message);
  return c;
}

const nameOf = (c: OwnedCard) =>
  c.card.kind === "name"
    ? c.card.name
    : (printings.get(`${c.card.set}/${c.card.num}`)?.name ?? "?");

describe("the collection's sections", () => {
  it("puts unsorted first, then every place, empty or not, cards by name", () => {
    const { places, cards } = parsed();
    const shown = sections(places, cards, nameOf).map((s) => [
      s.place?.name ?? null,
      s.qty,
      s.cards.map(nameOf),
    ]);
    expect(shown).toEqual([
      [null, 40, ["Island"]],
      ["Bulk", 4, ["Arcane Signet", "Sol Ring"]],
      ["Lantern", 1, ["Rashmi and Ragavan"]],
      ["Trade binder", 0, []],
    ]);
  });

  it("shows only where a filter matched", () => {
    const { places, cards } = parsed();
    const shown = sections(places, cards, nameOf, "an").map((s) => [
      s.place?.name ?? null,
      s.cards.map(nameOf),
    ]);
    expect(shown).toEqual([
      [null, ["Island"]],
      ["Bulk", ["Arcane Signet"]],
      ["Lantern", ["Rashmi and Ragavan"]],
    ]);
  });
});

describe("adding by name", () => {
  it("raises the line already holding the card there, printing or name", () => {
    const { cards } = parsed();
    const more = addOwnedByName(TEXT, cards, printings, "sol ring", "Bulk");
    expect(more).toContain(`{ name = "Sol Ring", qty = 4, at = "Bulk" },`);
    const again = addOwnedByName(
      TEXT,
      cards,
      printings,
      "Rashmi and Ragavan",
      "Lantern",
    );
    expect(again).toContain(
      `{ printing = "moc/94", qty = 2, at = "Lantern" },`,
    );
  });

  it("starts a line by name anywhere else", () => {
    const { cards } = parsed();
    const text = addOwnedByName(TEXT, cards, printings, "Sol Ring", null);
    const after = parseCollection(text);
    expect(after.kind === "collection" && after.total).toBe(46);
    expect(text).toContain(`  { name = "Sol Ring" },\n]`);
  });
});
