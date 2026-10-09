import type { Wanted } from "../deck.gen";

/**
 * Where the page's copy of Scryfall is (ADR-0030). Only `ready` answers; in
 * every other state a lookup goes to Scryfall's API instead, so a first
 * visit works while the copy is still downloading.
 */
export type CopyState =
  /** This browser cannot keep one: no worker, or no origin-private storage. */
  | { kind: "unavailable"; reason: string }
  /** Reading the copy kept from an earlier visit, which takes a second or two. */
  | { kind: "opening" }
  /** Making one: Default Cards so far, in compressed bytes. */
  | { kind: "downloading"; received: number; total: number }
  /** Made, and being written to storage. */
  | { kind: "saving" }
  /**
   * Answering, from Scryfall's bulk data written at `updatedAt`. `refresh` is
   * a newer copy being made meanwhile, which takes over when it is done.
   */
  | {
      kind: "ready";
      updatedAt: string;
      refresh?: { received: number; total: number };
    }
  /** No copy, and making one failed for this reason. */
  | { kind: "failed"; message: string };

export type Request =
  | { kind: "lookup"; wanted: Wanted[] }
  | { kind: "prints"; uri: string }
  | { kind: "search"; query: string; offset: number; limit: number }
  | { kind: "autocomplete"; query: string };

/** What the page sends the worker. */
export interface Asked {
  id: number;
  request: Request;
}

/** What the worker sends the page: an answer, as the copy's JSON, or its state. */
export type Told =
  | { kind: "answer"; id: number; json: string }
  | { kind: "error"; id: number; message: string }
  | { kind: "state"; state: CopyState };
