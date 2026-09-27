/**
 * Scryfall's rate limits, as one client-side queue per rate class
 * (scryfall-in-the-browser.md): 2 requests a second for search, named and
 * collection, 10 a second for everything else, autocomplete included. Every
 * call to api.scryfall.com goes through `scryfallFetch` with its class's gate,
 * so no feature can send a burst another feature's pacing does not know about.
 */

/**
 * Hands out start times at least `intervalMs` apart. A slot is reserved the
 * moment `wait` is called, so callers racing each other still queue in order.
 */
export class RateGate {
  private next = 0;
  constructor(
    readonly intervalMs: number,
    private readonly now: () => number = () => Date.now(),
  ) {}

  /** Resolves when this caller may start; rejects if `signal` aborts first. */
  async wait(signal?: AbortSignal): Promise<void> {
    const now = this.now();
    const at = Math.max(now, this.next);
    this.next = at + this.intervalMs;
    if (at > now) await new Promise((r) => setTimeout(r, at - now));
    signal?.throwIfAborted();
  }

  /** Nothing starts for `ms` from now: what a 429 asks of us. */
  pause(ms: number): void {
    this.next = Math.max(this.next, this.now() + ms);
  }
}

/** `/cards/search`, `/cards/named`, `/cards/random` and `/cards/collection`. */
export const SEARCH_GATE = new RateGate(500);
/** Everything else on api.scryfall.com, `/cards/autocomplete` included. */
export const AUTOCOMPLETE_GATE = new RateGate(100);

/** A 429 limits access "for 30 seconds", Scryfall says. */
export const BACKOFF_MS = 30_000;

export const API = "https://api.scryfall.com";

/**
 * `fetch`, started when `gate` allows. A 429 closes the gate for thirty
 * seconds and throws, since "it is not acceptable to ignore HTTP 429".
 */
export async function scryfallFetch(
  gate: RateGate,
  url: string,
  init: RequestInit = {},
): Promise<Response> {
  await gate.wait(init.signal ?? undefined);
  const response = await fetch(url, init);
  if (response.status === 429) {
    gate.pause(BACKOFF_MS);
    throw new Error("Scryfall asked us to slow down; trying again in 30 s");
  }
  return response;
}
