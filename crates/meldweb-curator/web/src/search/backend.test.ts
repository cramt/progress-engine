import { describe, expect, it } from "vitest";
import { readPage, SearchRefused } from "./backend";

const sol = {
  object: "card",
  id: "abc",
  name: "Sol Ring",
  type_line: "Artifact",
  mana_cost: "{1}",
  set: "cmm",
  collector_number: "410",
  image_uris: { normal: "https://cards.scryfall.io/sol.jpg" },
};
const bala = {
  object: "card",
  id: "def",
  name: "Bala Ged Recovery // Bala Ged Sanctuary",
  type_line: "Sorcery // Land",
  set: "znr",
  collector_number: "180",
  card_faces: [
    {
      mana_cost: "{2}{G}",
      image_uris: { normal: "https://cards.scryfall.io/bala.jpg" },
    },
    { mana_cost: "" },
  ],
};

const noFollow = () => Promise.reject(new Error("no follow expected"));

describe("reading a scryfall search answer", () => {
  it("takes the cards, the total, the warnings and the next page", async () => {
    const page = readPage(
      200,
      {
        object: "list",
        total_cards: 3863,
        has_more: true,
        next_page: "https://api.scryfall.com/cards/search?page=2",
        warnings: [
          "Invalid expression “foo:bar” was ignored. Unknown keyword “foo”.",
        ],
        data: [sol, bala, { object: "card" }],
      },
      async (next) => ({ cards: [], total: 0, warnings: [next] }),
    );
    expect(page.total).toBe(3863);
    expect(page.warnings).toEqual([
      "Invalid expression “foo:bar” was ignored. Unknown keyword “foo”.",
    ]);
    expect(page.cards).toEqual([
      {
        id: "abc",
        name: "Sol Ring",
        typeLine: "Artifact",
        manaCost: "{1}",
        image: "https://cards.scryfall.io/sol.jpg",
        set: "cmm",
        num: "410",
      },
      {
        id: "def",
        name: "Bala Ged Recovery // Bala Ged Sanctuary",
        typeLine: "Sorcery // Land",
        manaCost: "{2}{G}",
        image: "https://cards.scryfall.io/bala.jpg",
        set: "znr",
        num: "180",
      },
    ]);
    expect((await page.more?.())?.warnings).toEqual([
      "https://api.scryfall.com/cards/search?page=2",
    ]);
  });

  it("has no next page on the last one", () => {
    const page = readPage(
      200,
      { object: "list", total_cards: 1, has_more: false, data: [sol] },
      noFollow,
    );
    expect(page.more).toBeUndefined();
  });

  it("reads a 404 as no cards, keeping its warnings", () => {
    expect(
      readPage(
        404,
        { object: "error", code: "not_found", warnings: ["w"] },
        noFollow,
      ),
    ).toEqual({ cards: [], total: 0, warnings: ["w"] });
  });

  it("refuses a 400 with scryfall's details and warnings", () => {
    const refuse = () =>
      readPage(
        400,
        {
          object: "error",
          code: "bad_request",
          details: "All of your terms were ignored.",
          warnings: ["Invalid expression “is:slick” was ignored."],
        },
        noFollow,
      );
    expect(refuse).toThrow(SearchRefused);
    try {
      refuse();
    } catch (e) {
      expect((e as SearchRefused).message).toBe(
        "All of your terms were ignored.",
      );
      expect((e as SearchRefused).warnings).toHaveLength(1);
    }
  });
});
