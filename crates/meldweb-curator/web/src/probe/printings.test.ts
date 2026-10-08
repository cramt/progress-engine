import { afterEach, describe, expect, it, vi } from "vitest";
import { SEARCH_GATE } from "../scryfallQueue";
import { printingsById } from "./printings";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
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
