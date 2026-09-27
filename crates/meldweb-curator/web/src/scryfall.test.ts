import { afterEach, describe, expect, it, vi } from "vitest";
import type { Card } from "./deck";
import { COLLECTION_INTERVAL_MS, fetchPrintings } from "./scryfall";

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
    expect(COLLECTION_INTERVAL_MS).toBeGreaterThanOrEqual(500);
    expect(started).toHaveLength(3);
    for (let i = 1; i < started.length; i++)
      expect((started[i] ?? 0) - (started[i - 1] ?? 0)).toBeGreaterThanOrEqual(
        500,
      );
  });
});
