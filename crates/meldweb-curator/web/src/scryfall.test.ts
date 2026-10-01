import { afterEach, describe, expect, it, vi } from "vitest";
import type { Card } from "./deck";
import { fetchPrices, fetchPrintings } from "./scryfall";
import { SEARCH_GATE } from "./scryfallQueue";

const card = (i: number): Card => ({
  index: i,
  card: { kind: "name", name: `Card ${i}` },
  qty: 1,
  finish: "nonfoil",
  categories: [],
  place: "in-deck",
  inDeck: true,
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("collection lookups", () => {
  it("keep to scryfall's 2 requests a second", async () => {
    vi.useFakeTimers();
    const started: number[] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        started.push(Date.now());
        return new Response(JSON.stringify({ data: [] }));
      }),
    );
    // 160 distinct cards is three batches of at most 75.
    const done = fetchPrintings(Array.from({ length: 160 }, (_, i) => card(i)));
    await vi.runAllTimersAsync();
    await done;
    expect(SEARCH_GATE.intervalMs).toBeGreaterThanOrEqual(500);
    expect(started).toHaveLength(3);
    for (let i = 1; i < started.length; i++)
      expect((started[i] ?? 0) - (started[i - 1] ?? 0)).toBeGreaterThanOrEqual(
        500,
      );
  });

  it("ask scryfall only about the cards no earlier lookup found", async () => {
    const asked: string[][] = [];
    vi.stubGlobal(
      "fetch",
      vi.fn(async (_url: string, init?: RequestInit) => {
        const { identifiers } = JSON.parse(String(init?.body)) as {
          identifiers: { name: string }[];
        };
        asked.push(identifiers.map((i) => i.name));
        return new Response(
          JSON.stringify({
            data: identifiers.map(({ name }) => ({
              name,
              set: "tst",
              collector_number: name.slice(5),
              image_uris: { normal: `n/${name}` },
            })),
          }),
        );
      }),
    );
    await fetchPrintings([card(900), card(901)]);
    const again = await fetchPrintings([card(901), card(902)]);
    expect(asked).toEqual([["Card 900", "Card 901"], ["Card 902"]]);
    expect(again.get("name:card 901")?.image).toBe("n/Card 901");
    expect(again.get("name:card 902")?.image).toBe("n/Card 902");
  });
});

describe("price lookups", () => {
  it("read today's prices off any collection lookup, by finish", async () => {
    let requests = 0;
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        requests++;
        return new Response(
          JSON.stringify({
            data: [
              {
                name: "Card 950",
                set: "tst",
                collector_number: "950",
                image_uris: { normal: "n/950" },
                prices: { eur: "1.25", eur_foil: null, usd: "2.00" },
              },
            ],
          }),
        );
      }),
    );
    await fetchPrintings([card(950)]);
    const prices = await fetchPrices([card(950)]);
    expect(requests).toBe(1);
    expect(prices.get("name:card 950")).toEqual({
      eur: { nonfoil: 1.25 },
      usd: { nonfoil: 2 },
    });
  });
});
