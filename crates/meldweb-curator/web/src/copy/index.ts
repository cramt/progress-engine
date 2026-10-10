/**
 * The page's side of its copy of Scryfall (ADR-0030): the worker holding it,
 * and `viaCopy`, which every card lookup goes through so it is answered from
 * the copy when there is one and from Scryfall's API when there is not yet.
 */
import { useSyncExternalStore } from "react";
import type { Found, PrintingFacts, SearchAnswer, Wanted } from "../deck.gen";
import type { Asked, CopyState, Request, Told } from "./protocol";

export type { CopyState } from "./protocol";

/** What the copy answers, once it is ready. */
export interface Copy {
  lookup(wanted: readonly Wanted[]): Promise<(Found | null)[]>;
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
  worker.onerror = (e) => {
    const message = e.message || "the copy's worker failed";
    set({ kind: "failed", message });
    // A dead worker answers nothing more, so what it was asked goes to the
    // API rather than waiting for ever.
    for (const pending of waiting.values()) pending.reject(new Error(message));
    waiting.clear();
  };
}

export function copyState(): CopyState {
  return state;
}

/** Calls `listener` on every change of state until the returned function. */
export function watchCopy(listener: (state: CopyState) => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** The copy's state, rendered again whenever it changes. */
export function useCopyState(): CopyState {
  return useSyncExternalStore((changed) => {
    const stop = watchCopy(changed);
    return () => void stop();
  }, copyState);
}

/**
 * How long a question waits for the worker before the API is asked instead.
 * The worker says it is `busy` before the seconds it spends reading or
 * finishing a whole copy, so nothing is asked of it then; between those, the
 * longest it is busy is feeding a chunk of Scryfall's file, about a second.
 */
const ASK_MS = 10_000;

function ask<T>(request: Request): Promise<T> {
  const w = worker;
  if (!w) return Promise.reject(new Error("the copy is not running"));
  const id = nextId++;
  return new Promise<string>((resolve, reject) => {
    const timer = setTimeout(() => {
      waiting.delete(id);
      reject(new Error(`the copy did not answer in ${ASK_MS / 1000} s`));
    }, ASK_MS);
    waiting.set(id, {
      resolve: (json) => {
        clearTimeout(timer);
        resolve(json);
      },
      reject: (e) => {
        clearTimeout(timer);
        reject(e);
      },
    });
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

function answering(): boolean {
  return state.kind === "ready" && !state.busy;
}

/**
 * `local` against the copy, or `api` when the copy cannot answer yet or
 * fails, so no lookup is lost to it. A copy still being read back is not
 * waited for: the API and its cache answer a page load as they did before
 * there was a copy, and the copy takes over once it is ready.
 *
 * An answer `found` says is empty goes to the API too: the copy can be a day
 * behind Scryfall, and Scryfall matches names more loosely (`Juzam Djinn`
 * finds Juzám Djinn), so the copy having nothing is not Scryfall having
 * nothing. When that asking fails, offline say, the copy's answer stands.
 */
export async function viaCopy<T>(
  local: (copy: Copy) => Promise<T>,
  api: () => Promise<T>,
  found: (answer: T) => boolean = () => true,
): Promise<T> {
  if (!answering()) return api();
  let answer: T;
  try {
    answer = await local(copy);
  } catch (e) {
    console.warn("the copy of Scryfall could not answer; asking Scryfall", e);
    return api();
  }
  if (found(answer)) return answer;
  return api().catch(() => answer);
}

/**
 * `viaCopy` for many things at once, keyed by `key`: what the copy has, and
 * the API asked for only what it has not, for the reasons `viaCopy` gives.
 */
export async function viaCopyEach<W, T>(
  wanted: readonly W[],
  key: (w: W) => string,
  local: (copy: Copy, wanted: readonly W[]) => Promise<ReadonlyMap<string, T>>,
  api: (wanted: readonly W[]) => Promise<ReadonlyMap<string, T>>,
): Promise<ReadonlyMap<string, T>> {
  if (!answering()) return api(wanted);
  let had: ReadonlyMap<string, T>;
  try {
    had = await local(copy, wanted);
  } catch (e) {
    console.warn("the copy of Scryfall could not answer; asking Scryfall", e);
    return api(wanted);
  }
  const missed = wanted.filter((w) => !had.has(key(w)));
  if (missed.length === 0) return had;
  try {
    return new Map([...had, ...(await api(missed))]);
  } catch {
    return had;
  }
}
