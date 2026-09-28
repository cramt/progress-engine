import { afterEach, describe, expect, it, vi } from "vitest";
import { BACKOFF_MS, RateGate, scryfallFetch } from "./scryfallQueue";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

/** Every start time, when `count` callers race for `gate` at once. */
async function starts(gate: RateGate, count: number): Promise<number[]> {
  const at: number[] = [];
  const all = Array.from({ length: count }, () =>
    gate.wait().then(() => at.push(Date.now())),
  );
  await vi.runAllTimersAsync();
  await Promise.all(all);
  return at;
}

describe("a rate gate", () => {
  it("lets the first caller straight through", async () => {
    vi.useFakeTimers();
    const t0 = Date.now();
    expect(await starts(new RateGate(500), 1)).toEqual([t0]);
  });

  it("spaces racing callers at least its interval apart", async () => {
    vi.useFakeTimers();
    const t0 = Date.now();
    const at = await starts(new RateGate(100), 25);
    expect(at).toHaveLength(25);
    for (let i = 1; i < at.length; i++)
      expect((at[i] ?? 0) - (at[i - 1] ?? 0)).toBeGreaterThanOrEqual(100);
    // So no one-second window holds more than ten.
    for (const s of at)
      expect(
        at.filter((x) => x >= s && x < s + 1000).length,
      ).toBeLessThanOrEqual(10);
    expect(at[0]).toBe(t0);
  });

  it("does not make a caller wait who comes after a quiet interval", async () => {
    vi.useFakeTimers();
    const gate = new RateGate(500);
    await starts(gate, 1);
    await vi.advanceTimersByTimeAsync(2000);
    const t = Date.now();
    expect(await starts(gate, 1)).toEqual([t]);
  });

  it("rejects a caller whose signal aborted while it waited", async () => {
    vi.useFakeTimers();
    const gate = new RateGate(500);
    await gate.wait();
    const controller = new AbortController();
    const waiting = gate.wait(controller.signal);
    controller.abort();
    const settled = waiting.then(
      () => "started",
      () => "aborted",
    );
    await vi.runAllTimersAsync();
    expect(await settled).toBe("aborted");
  });

  it("holds a caller already waiting through a pause", async () => {
    vi.useFakeTimers();
    const t0 = Date.now();
    const gate = new RateGate(500);
    await gate.wait();
    let started = 0;
    const waiting = gate.wait().then(() => {
      started = Date.now();
    });
    await vi.advanceTimersByTimeAsync(100);
    gate.pause(BACKOFF_MS);
    await vi.runAllTimersAsync();
    await waiting;
    expect(started - t0).toBeGreaterThanOrEqual(100 + BACKOFF_MS);
  });

  it("rejects an aborted caller at once, and gives its slot to the next", async () => {
    vi.useFakeTimers();
    const t0 = Date.now();
    const gate = new RateGate(500);
    await gate.wait();
    const controller = new AbortController();
    const aborted = gate.wait(controller.signal).then(
      () => "started",
      () => "aborted",
    );
    let started = 0;
    const after = gate.wait().then(() => {
      started = Date.now();
    });
    controller.abort();
    // No time passes: the aborted caller does not sit out its turn.
    await vi.advanceTimersByTimeAsync(0);
    expect(await aborted).toBe("aborted");
    await vi.runAllTimersAsync();
    await after;
    expect(started - t0).toBe(500);
  });

  it("rejects a caller whose signal aborted before it asked", async () => {
    await expect(new RateGate(500).wait(AbortSignal.abort())).rejects.toThrow();
  });
});

describe("scryfallFetch", () => {
  it("closes the gate for thirty seconds after a 429", async () => {
    vi.useFakeTimers();
    const gate = new RateGate(100);
    const started: number[] = [];
    let status = 429;
    vi.stubGlobal(
      "fetch",
      vi.fn(async () => {
        started.push(Date.now());
        return new Response("{}", { status });
      }),
    );
    await expect(scryfallFetch(gate, "https://x/")).rejects.toThrow(
      /slow down/,
    );
    status = 200;
    const next = scryfallFetch(gate, "https://x/");
    await vi.runAllTimersAsync();
    expect((await next).status).toBe(200);
    expect((started[1] ?? 0) - (started[0] ?? 0)).toBeGreaterThanOrEqual(
      BACKOFF_MS,
    );
  });
});
