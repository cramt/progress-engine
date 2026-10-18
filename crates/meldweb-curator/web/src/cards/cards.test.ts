import { describe, expect, it } from "vitest";
import { besideCard, cardPath, cardStem, isCardPath } from "../github/cards";
import { commitMessageFor } from "../github/repoFile";
import { atCost } from "./real";

describe("a custom card's file", () => {
  it("is named from the card, in cards/", () => {
    expect(cardPath("Proofsheet Drone")).toBe(
      "cards/proofsheet-drone.card.toml",
    );
    expect(cardPath("Jötun's Ål, Grunt")).toBe(
      "cards/jotun-s-al-grunt.card.toml",
    );
    expect(cardStem("cards/proofsheet-drone.card.toml")).toBe(
      "proofsheet-drone",
    );
    expect(isCardPath("cards/x.card.toml")).toBe(true);
    expect(isCardPath("decks/x.deck.toml")).toBe(false);
  });

  it("names its art relative to itself", () => {
    expect(besideCard("cards/a.card.toml", "art/a.png")).toBe(
      "cards/art/a.png",
    );
    expect(besideCard("cards/a.card.toml", "../assets/a.jpg")).toBe(
      "assets/a.jpg",
    );
  });

  it("is committed with a message naming it", () => {
    expect(commitMessageFor("cards/a.card.toml", "", "name = 1")).toBe(
      "a: custom card made",
    );
    expect(commitMessageFor("cards/a.card.toml", "x", "y")).toBe(
      "a: custom card edited",
    );
  });
});

describe("the real cards a custom one is compared with", () => {
  it("are the faces at exactly its cost", () => {
    const found = [
      { name: "Opt", mana_cost: "{U}", oracle_text: "Scry 1.\nDraw a card." },
      { name: "Brainstorm", mana_cost: "{U}{U}", oracle_text: "Draw" },
      {
        name: "Delver of Secrets // Insectile Aberration",
        card_faces: [
          {
            name: "Delver of Secrets",
            mana_cost: "{U}",
            oracle_text: "At the beginning",
          },
          {
            name: "Insectile Aberration",
            mana_cost: "",
            oracle_text: "Flying",
          },
        ],
      },
    ];
    expect(atCost(found, "{U}").map((r) => r.name)).toEqual([
      "Opt",
      "Delver of Secrets",
    ]);
  });
});
