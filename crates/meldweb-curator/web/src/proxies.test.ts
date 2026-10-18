import { readFileSync } from "node:fs";
import { beforeAll, describe, expect, it } from "vitest";
import { loadDeckSync } from "./deck";
import { perPage, proxiesOf, sideImages, toPrint } from "./proxies";
import type { Printing } from "./scryfall";

beforeAll(() => {
  loadDeckSync(
    readFileSync(
      new URL("../src/wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url),
    ),
  );
});

const DECK = `cards = [
  { name = "Sol Ring" },
  { printing = "mid/51" },  # Delver of Secrets
  { name = "Island", qty = 2 },
]
`;
const PATH = "decks/a.deck.toml";

const scry = (id: string) => `https://cards.scryfall.io/normal/front/${id}.jpg`;

describe("what a deck has left to print", () => {
  it("is the whole deck with no collection", () => {
    const p = toPrint(PATH, DECK, null, {}, false);
    expect(p).toEqual({
      kind: "toPrint",
      lines: [
        { index: 0, qty: 1 },
        { index: 1, qty: 1 },
        { index: 2, qty: 2 },
      ],
      basics: 0,
      owned: 0,
    });
  });

  it("leaves out what the collection owns, in any printing, and the basics on request", () => {
    const mine = `cards = [{ printing = "mid/51" }]  # Delver of Secrets\n`;
    const p = toPrint(
      PATH,
      DECK,
      mine,
      { "mid/51": "Delver of Secrets // Insectile Aberration" },
      true,
    );
    expect(p).toEqual({
      kind: "toPrint",
      lines: [{ index: 0, qty: 1 }],
      basics: 2,
      owned: 1,
    });
  });

  it("fits nine to an A4 page", () => {
    expect(
      perPage({ paper: "a4", gapMm: 0, marginMm: 5, cutMarks: true }),
    ).toBe(9);
  });
});

describe("a proxy's pictures", () => {
  const delver = {
    image: scry("a"),
    back: { image: scry("b").replace("/front/", "/back/"), turn: "upright" },
  } as const;

  it("are Scryfall's large images, both sides of a card with two", () => {
    expect(sideImages(delver, "both")).toEqual([
      "https://cards.scryfall.io/large/front/a.jpg",
      "https://cards.scryfall.io/large/back/b.jpg",
    ]);
    expect(sideImages(delver, "front")).toHaveLength(1);
  });

  it("print a flip card once, its halves sharing a picture", () => {
    expect(
      sideImages({ image: scry("f"), back: { image: scry("f") } }, "both"),
    ).toHaveLength(1);
  });

  it("stop the sheet for a card Scryfall has not answered for", () => {
    const printings = new Map<string, Printing>([
      [
        "name:sol ring",
        {
          id: "x",
          name: "Sol Ring",
          set: "cmm",
          num: "400",
          image: scry("s"),
          colorIdentity: [],
          typeLine: "Artifact",
        },
      ],
    ]);
    const cards = [
      { index: 0, card: { kind: "name", name: "Sol Ring" } },
      { index: 1, card: { kind: "printing", set: "mid", num: "51" } },
    ] as const;
    expect(proxiesOf([{ index: 0, qty: 1 }], cards, printings, "both")).toEqual(
      {
        kind: "proxies",
        proxies: [
          {
            name: "Sol Ring",
            qty: 1,
            images: ["https://cards.scryfall.io/large/front/s.jpg"],
          },
        ],
      },
    );
    expect(
      proxiesOf(
        [
          { index: 0, qty: 1 },
          { index: 1, qty: 1 },
        ],
        cards,
        printings,
        "both",
      ),
    ).toEqual({ kind: "unknown", names: ["mid/51"] });
  });
});
