import { viaCopy } from "../copy";
import { cachedOne } from "../scryfallCache";
import { API, AUTOCOMPLETE_GATE, scryfallFetch } from "../scryfallQueue";

/** Looks up the names a prefix could be; aborting `signal` abandons it. */
export type Lookup = (query: string, signal: AbortSignal) => Promise<string[]>;

/**
 * Up to 20 names, nearest first: from the page's copy of Scryfall, or before
 * there is one from Scryfall's `/cards/autocomplete`, paced by the
 * 10-a-second queue and cached per prefix, so typing back over a prefix asks
 * nothing. Below two characters Scryfall answers an empty catalog, so we do
 * not ask.
 */
export const scryfallAutocomplete: Lookup = (query, signal) =>
  viaCopy(
    (copy) => copy.autocomplete(query),
    () => apiAutocomplete(query, signal),
  );

const apiAutocomplete: Lookup = (query, signal) =>
  cachedOne(["scryfall", "autocomplete", query], async () => {
    const response = await scryfallFetch(
      AUTOCOMPLETE_GATE,
      `${API}/cards/autocomplete?q=${encodeURIComponent(query)}`,
      { signal },
    );
    if (!response.ok) throw new Error(`Scryfall answered ${response.status}`);
    const data = ((await response.json()) as { data?: unknown }).data;
    return Array.isArray(data)
      ? data.filter((n): n is string => typeof n === "string")
      : [];
  });

export const DEBOUNCE_MS = 150;
export const MIN_LENGTH = 2;

/**
 * Quick add's typing, turned into as few lookups as keep the list current:
 * a lookup starts once typing has paused for `delayMs`, and only the answer to
 * the latest input is ever shown. An older answer arriving late is dropped,
 * and its request aborted if it has not finished.
 */
export function createAutocomplete({
  lookup,
  onResults,
  onError,
  delayMs = DEBOUNCE_MS,
  minLength = MIN_LENGTH,
}: {
  lookup: Lookup;
  /** The names for `query`, which is still what the box holds. */
  onResults: (query: string, names: string[]) => void;
  onError?: (error: unknown) => void;
  delayMs?: number;
  minLength?: number;
}) {
  let timer: ReturnType<typeof setTimeout> | undefined;
  let inFlight: AbortController | undefined;
  let latest = 0;

  const cancel = () => {
    clearTimeout(timer);
    inFlight?.abort();
    inFlight = undefined;
    latest++;
  };

  const input = (raw: string) => {
    cancel();
    const query = raw.trim();
    if (query.length < minLength) {
      onResults(query, []);
      return;
    }
    const mine = latest;
    timer = setTimeout(() => {
      const controller = new AbortController();
      inFlight = controller;
      lookup(query, controller.signal).then(
        (names) => {
          if (mine === latest) onResults(query, names);
        },
        (error: unknown) => {
          if (mine === latest && !controller.signal.aborted) onError?.(error);
        },
      );
    }, delayMs);
  };

  return { input, cancel };
}
