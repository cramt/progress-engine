import { afterEach, describe, expect, it, vi } from "vitest";
import { createAutocomplete, DEBOUNCE_MS, type Lookup } from "./autocomplete";

afterEach(() => vi.useRealTimers());

/** A lookup whose answers the test releases by hand. */
function controlledLookup() {
  const calls: {
    query: string;
    signal: AbortSignal;
    answer: (names: string[]) => void;
  }[] = [];
  const lookup: Lookup = (query, signal) =>
    new Promise((resolve) => calls.push({ query, signal, answer: resolve }));
  return { calls, lookup };
}

describe("quick add's autocomplete", () => {
  it("looks up once typing pauses, not per keystroke", async () => {
    vi.useFakeTimers();
    const { calls, lookup } = controlledLookup();
    const auto = createAutocomplete({ lookup, onResults: () => {} });
    // Typing "sol ri" at 60 ms a keystroke, faster than any typist.
    for (const prefix of ["s", "so", "sol", "sol ", "sol r", "sol ri"]) {
      auto.input(prefix);
      await vi.advanceTimersByTimeAsync(60);
    }
    expect(calls).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(calls.map((c) => c.query)).toEqual(["sol ri"]);
  });

  it("never asks about fewer than two characters", async () => {
    vi.useFakeTimers();
    const { calls, lookup } = controlledLookup();
    const results = vi.fn();
    const auto = createAutocomplete({ lookup, onResults: results });
    auto.input(" s ");
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toHaveLength(0);
    expect(results).toHaveBeenCalledWith("s", []);
  });

  it("drops an answer that arrives after newer typing, and aborts its request", async () => {
    vi.useFakeTimers();
    const { calls, lookup } = controlledLookup();
    const results = vi.fn();
    const auto = createAutocomplete({ lookup, onResults: results });
    auto.input("sol");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    auto.input("sol ri");
    await vi.advanceTimersByTimeAsync(DEBOUNCE_MS);
    expect(calls.map((c) => c.query)).toEqual(["sol", "sol ri"]);
    expect(calls[0]?.signal.aborted).toBe(true);
    // The newer answer lands first, then the stale one.
    calls[1]?.answer(["Sol Ring"]);
    calls[0]?.answer(["Sol Talisman", "Sol Ring"]);
    await vi.runAllTimersAsync();
    expect(results.mock.calls).toEqual([["sol ri", ["Sol Ring"]]]);
  });

  it("forgets a pending lookup when cancelled", async () => {
    vi.useFakeTimers();
    const { calls, lookup } = controlledLookup();
    const auto = createAutocomplete({ lookup, onResults: () => {} });
    auto.input("sol ri");
    auto.cancel();
    await vi.advanceTimersByTimeAsync(1000);
    expect(calls).toHaveLength(0);
  });
});
