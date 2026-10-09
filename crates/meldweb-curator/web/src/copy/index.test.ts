import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import type { Asked, Told } from "./protocol";

/**
 * A worker that is ready at once and answers each lookup with `answers`, or
 * whatever `answer` makes of a request; one asked for `"crash"` dies.
 */
class FakeWorker {
  static last: FakeWorker;
  onmessage: ((e: MessageEvent<Told>) => void) | null = null;
  onerror: ((e: ErrorEvent) => void) | null = null;
  answer: (a: Asked) => unknown = () => [];
  constructor() {
    FakeWorker.last = this;
    queueMicrotask(() =>
      this.tell({
        kind: "state",
        state: { kind: "ready", updatedAt: "2026-10-09" },
      }),
    );
  }
  tell(data: Told) {
    this.onmessage?.({ data } as MessageEvent<Told>);
  }
  postMessage(asked: Asked) {
    if (
      asked.request.kind === "autocomplete" &&
      asked.request.query === "crash"
    )
      return queueMicrotask(() =>
        this.onerror?.({ message: "out of memory" } as ErrorEvent),
      );
    queueMicrotask(() =>
      this.tell({
        kind: "answer",
        id: asked.id,
        json: JSON.stringify(this.answer(asked)),
      }),
    );
  }
}

describe("viaCopy", () => {
  let copy: typeof import("./index");
  beforeAll(async () => {
    vi.stubGlobal("Worker", FakeWorker);
    copy = await import("./index");
    copy.startCopy();
    await vi.waitFor(() => expect(copy.copyState().kind).toBe("ready"));
  });
  afterAll(() => vi.unstubAllGlobals());

  it("asks the API for only what the copy did not find", async () => {
    FakeWorker.last.answer = () => [{ name: "Sol Ring" }, null];
    const api = vi.fn(async (names: readonly string[]) => {
      return new Map(names.map((n) => [n, `api ${n}`]));
    });
    const found = await copy.viaCopyEach(
      ["Sol Ring", "Juzam Djinn"],
      (n) => n,
      async (c, names) => {
        const found = await c.lookup(
          names.map((name) => ({ kind: "name", name })),
        );
        return new Map(
          names.flatMap((n, i) => (found[i] ? [[n, "copy"] as const] : [])),
        );
      },
      api,
    );
    expect(api).toHaveBeenCalledWith(["Juzam Djinn"]);
    expect([...found]).toEqual([
      ["Sol Ring", "copy"],
      ["Juzam Djinn", "api Juzam Djinn"],
    ]);
  });

  it("asks the API when the copy has nothing, and keeps the copy's nothing when the API fails", async () => {
    FakeWorker.last.answer = () => [];
    const found = (names: string[]) => names.length > 0;
    expect(
      await copy.viaCopy(
        (c) => c.autocomplete("juzam"),
        async () => ["Juzám Djinn"],
        found,
      ),
    ).toEqual(["Juzám Djinn"]);
    expect(
      await copy.viaCopy(
        (c) => c.autocomplete("juzam"),
        () => Promise.reject(new Error("offline")),
        found,
      ),
    ).toEqual([]);
  });

  it("asks the API, not the copy, while the worker is busy", async () => {
    const ready = { kind: "ready", updatedAt: "2026-10-09" } as const;
    FakeWorker.last.tell({ kind: "state", state: { ...ready, busy: true } });
    const local = vi.fn(async () => ["from the copy"]);
    expect(await copy.viaCopy(local, async () => ["from the API"])).toEqual([
      "from the API",
    ]);
    expect(local).not.toHaveBeenCalled();
    FakeWorker.last.tell({ kind: "state", state: ready });
    expect(await copy.viaCopy(local, async () => ["from the API"])).toEqual([
      "from the copy",
    ]);
  });

  it("asks the API for what a dying worker was asked", async () => {
    vi.spyOn(console, "warn").mockImplementation(() => {});
    expect(
      await copy.viaCopy(
        (c) => c.autocomplete("crash"),
        async () => ["from the API"],
      ),
    ).toEqual(["from the API"]);
    expect(copy.copyState().kind).toBe("failed");
  });
});
