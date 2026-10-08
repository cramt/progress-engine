/**
 * Scryfall's rate limits, as one client-side queue per rate class
 * (scryfall-in-the-browser.md): 2 requests a second for search, named and
 * collection, 10 a second for everything else, autocomplete included. Every
 * call to api.scryfall.com goes through `scryfallFetch` with its class's gate,
 * so no feature can send a burst another feature's pacing does not know about.
 */

interface Waiter {
  start: () => void;
}

/**
 * Lets callers start at least `intervalMs` apart, in the order they called
 * `wait`. Waiters are a queue rather than start times handed out up front, so
 * a pause holds back everyone still waiting, and a caller who gives up leaves
 * its turn to the next.
 */
export class RateGate {
  /** The earliest the next caller may start. */
  private next = 0;
  private readonly queue: Waiter[] = [];
  private timer: ReturnType<typeof setTimeout> | undefined;
  constructor(
    readonly intervalMs: number,
    private readonly now: () => number = () => Date.now(),
  ) {}

  /** Resolves when this caller may start; rejects as soon as `signal` aborts. */
  wait(signal?: AbortSignal): Promise<void> {
    return new Promise((resolve, reject) => {
      if (signal?.aborted) return reject(signal.reason);
      const onAbort = () => {
        const at = this.queue.indexOf(waiter);
        if (at >= 0) this.queue.splice(at, 1);
        reject(signal?.reason);
      };
      const waiter: Waiter = {
        start: () => {
          signal?.removeEventListener("abort", onAbort);
          resolve();
        },
      };
      signal?.addEventListener("abort", onAbort, { once: true });
      this.queue.push(waiter);
      this.drain();
    });
  }

  /** Nothing starts for `ms` from now: what a 429 asks of us. */
  pause(ms: number): void {
    this.next = Math.max(this.next, this.now() + ms);
    this.drain();
  }

  /** Starts whoever may start now, and wakes up again for the rest. */
  private drain(): void {
    clearTimeout(this.timer);
    this.timer = undefined;
    for (let waiter = this.queue[0]; waiter; waiter = this.queue[0]) {
      const now = this.now();
      if (now < this.next) {
        this.timer = setTimeout(() => this.drain(), this.next - now);
        return;
      }
      this.queue.shift();
      this.next = now + this.intervalMs;
      waiter.start();
    }
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
 *
 * A 429 can also arrive as no response at all: Scryfall sends it without
 * CORS headers, so the browser reports only that the fetch failed. That is
 * taken as a 429 too, closing the gate, and the request is made once more
 * after it, since an import of thousands of printings sends enough requests
 * that one landing too close to the last is a matter of time, and losing the
 * other batches to it would lose the import.
 */
export async function scryfallFetch(
  gate: RateGate,
  url: string,
  init: RequestInit = {},
): Promise<Response> {
  await gate.wait(init.signal ?? undefined);
  let response: Response;
  try {
    response = await fetch(url, init);
  } catch (e) {
    if (init.signal?.aborted) throw e;
    gate.pause(BACKOFF_MS);
    await gate.wait(init.signal ?? undefined);
    response = await fetch(url, init);
  }
  if (response.status === 429) {
    gate.pause(BACKOFF_MS);
    throw new Error("Scryfall asked us to slow down; trying again in 30 s");
  }
  return response;
}
