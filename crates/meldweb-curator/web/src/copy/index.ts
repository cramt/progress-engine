/**
 * The page's side of its copy of Scryfall (ADR-0030): the worker holding it,
 * and `viaCopy`, which every card lookup goes through so it is answered from
 * the copy when there is one and from Scryfall's API when there is not yet.
 */
import type { Found, PrintingFacts, SearchAnswer, Wanted } from "../deck.gen";
import type { Asked, CopyState, Request, Told } from "./protocol";

export type { CopyState } from "./protocol";

/** What the copy answers, once it is ready. */
export interface Copy {
  lookup(wanted: Wanted[]): Promise<(Found | null)[]>;
  /** Every printing behind a search for a card's printings, newest first. */
  prints(uri: string): Promise<PrintingFacts[]>;
  search(query: string, offset: number, limit: number): Promise<SearchAnswer>;
  autocomplete(query: string): Promise<string[]>;
}

let worker: Worker | null = null;
let state: CopyState = { kind: "unavailable", reason: "not started" };
const listeners = new Set<(state: CopyState) => void>();
const waiting = new Map<
  number,
  { resolve: (json: string) => void; reject: (e: Error) => void }
>();
let nextId = 0;

function set(next: CopyState): void {
  state = next;
  for (const l of listeners) l(state);
}

/**
 * Starts the worker, once, at page load. Where there is no worker (tests,
 * Node) the copy stays unavailable and every lookup goes to the API.
 */
export function startCopy(): void {
  if (worker || typeof Worker === "undefined") return;
  worker = new Worker(new URL("./worker.ts", import.meta.url), {
    type: "module",
  });
  set({ kind: "opening" });
  worker.onmessage = ({ data }: MessageEvent<Told>) => {
    if (data.kind === "state") return set(data.state);
    const pending = waiting.get(data.id);
    waiting.delete(data.id);
    if (data.kind === "answer") pending?.resolve(data.json);
    else pending?.reject(new Error(data.message));
  };
  worker.onerror = (e) =>
    set({ kind: "failed", message: e.message || "the copy's worker failed" });
}

export function copyState(): CopyState {
  return state;
}

/** Calls `listener` on every change of state until the returned function. */
export function watchCopy(listener: (state: CopyState) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

function ask<T>(request: Request): Promise<T> {
  const w = worker;
  if (!w) return Promise.reject(new Error("the copy is not running"));
  const id = nextId++;
  return new Promise<string>((resolve, reject) => {
    waiting.set(id, { resolve, reject });
    w.postMessage({ id, request } satisfies Asked);
  }).then((json) => JSON.parse(json) as T);
}

// The JSON is generated from the Rust side's types, which meldweb-wasm's
// tests pin deck.gen.ts to, so these casts are where the two are trusted.
const copy: Copy = {
  lookup: (wanted) => ask({ kind: "lookup", wanted }),
  prints: (uri) => ask({ kind: "prints", uri }),
  search: (query, offset, limit) =>
    ask({ kind: "search", query, offset, limit }),
  autocomplete: (query) => ask({ kind: "autocomplete", query }),
};

/**
 * The copy, once it can answer: at once when ready, after it is read back
 * when it is being opened, which is a second or two, and `null` when there
 * is none to wait for.
 */
function ready(): Promise<Copy | null> {
  if (state.kind === "ready") return Promise.resolve(copy);
  if (state.kind !== "opening") return Promise.resolve(null);
  return new Promise((resolve) => {
    const stop = watchCopy((s) => {
      if (s.kind === "opening") return;
      stop();
      resolve(s.kind === "ready" ? copy : null);
    });
  });
}

/**
 * `local` against the copy, or `api` when there is no copy to ask or the
 * copy fails, so no lookup is lost to it.
 */
export async function viaCopy<T>(
  local: (copy: Copy) => Promise<T>,
  api: () => Promise<T>,
): Promise<T> {
  const c = await ready();
  if (!c) return api();
  try {
    return await local(c);
  } catch (e) {
    console.warn("the copy of Scryfall could not answer; asking Scryfall", e);
    return api();
  }
}
