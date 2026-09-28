import { readFileSync } from "node:fs";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { loadDeckSync, parseDeck } from "../deck";
import { SEARCH_GATE } from "../scryfallQueue";
import { addPrinting, printingsById } from "./printings";

beforeAll(() => {
  loadDeckSync(
    readFileSync(new URL("../wasm/pkg/meldweb_wasm_bg.wasm", import.meta.url)),
  );
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

const deck = `name = "Test"

cards = [
  { printing = "lea/232", in = ["Maybe"] },  # Black Lotus
  { printing = "m21/159", in = ["Burn"] },  # Shock
]

[categories]
Burn = {}
Maybe = { type = "maybeboard" }
`;

function add(text: string, set: string, num: string, name: string) {
  const parsed = parseDeck(text);
  if (parsed.kind !== "deck") throw new Error(parsed.message);
  return addPrinting(text, parsed.cards, { set, num, name });
}

describe("adding a scanned printing", () => {
  it("adds a copy to the line that already names it", () => {
    const after = add(deck, "m21", "159", "Shock");
    expect(after).toContain(`{ printing = "m21/159", qty = 2, in = ["Burn"] }`);
  });

  it("matches the printing whatever its case", () => {
    expect(add(deck, "M21", "159", "Shock")).toContain("qty = 2");
  });

  it("makes a new line by printing for one the deck lacks", () => {
    const after = add(deck, "ths", "107", "Thoughtseize");
    const parsed = parseDeck(after);
    if (parsed.kind !== "deck") throw new Error(parsed.message);
    const added = parsed.cards.find(
      (c) => c.card.kind === "printing" && c.card.set === "ths",
    );
    expect(added).toMatchObject({ qty: 1, categories: [], inDeck: true });
    expect(after).toContain("# Thoughtseize");
  });
});

describe("looking scanned ids up on scryfall", () => {
  it("asks once, by id, and keys the answer by id", async () => {
    vi.useFakeTimers();
    const bodies: unknown[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init: RequestInit) => {
        bodies.push(JSON.parse(String(init.body)));
        return new Response(
          JSON.stringify({
            data: [
              {
                id: "b0faa7f2-b547-42c4-a810-839da50dadfe",
                name: "Black Lotus",
                set: "LEA",
                collector_number: "232",
                image_uris: { normal: "https://img/lotus.jpg" },
              },
            ],
            not_found: [{ id: "00000000-0000-0000-0000-000000000000" }],
          }),
        );
      }),
    );
    const done = printingsById([
      "b0faa7f2-b547-42c4-a810-839da50dadfe",
      "b0faa7f2-b547-42c4-a810-839da50dadfe",
      "00000000-0000-0000-0000-000000000000",
    ]);
    await vi.runAllTimersAsync();
    const found = await done;
    expect(SEARCH_GATE.intervalMs).toBeGreaterThan(0);
    expect(bodies).toEqual([
      {
        identifiers: [
          { id: "b0faa7f2-b547-42c4-a810-839da50dadfe" },
          { id: "00000000-0000-0000-0000-000000000000" },
        ],
      },
    ]);
    expect(found.get("b0faa7f2-b547-42c4-a810-839da50dadfe")).toEqual({
      set: "lea",
      num: "232",
      name: "Black Lotus",
      image: "https://img/lotus.jpg",
    });
    expect(found.size).toBe(1);
  });
});
