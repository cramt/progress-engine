/**
 * Every answer from api.scryfall.com, kept by TanStack Query and never asked
 * for twice. A card's printing, picture and name do not change under us, so
 * nothing here goes stale or is collected: an answer stays for the life of
 * the tab, and outlives it in IndexedDB, dropped only when the page has not
 * been opened for CACHE_MS.
 */
import { createAsyncStoragePersister } from "@tanstack/query-async-storage-persister";
import { QueryClient } from "@tanstack/react-query";
import {
  persistQueryClientRestore,
  persistQueryClientSave,
} from "@tanstack/react-query-persist-client";

const DAY_MS = 86_400_000;
export const CACHE_MS = 90 * DAY_MS;

export const scryfallClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: Number.POSITIVE_INFINITY,
      gcTime: Number.POSITIVE_INFINITY,
      // scryfallQueue already paces every request and backs off on a 429;
      // a retry on top would only spend the budget it guards.
      retry: false,
      refetchOnWindowFocus: false,
      refetchOnReconnect: false,
    },
  },
});

/**
 * Values for `wanted`, each cached on its own under `[...prefix, key]`, so a
 * card looked up once is found again by any later lookup that names it. The
 * misses alone go to `fetchMany`, together, which is where the batching lives;
 * it answers by key, and may answer keys nobody asked for (a card found by
 * name is also its printing), which are cached too. What it did not find is
 * absent, and asked again next time.
 */
export async function cachedMany<W, T>(
  prefix: readonly string[],
  wanted: readonly W[],
  keyOf: (w: W) => string,
  fetchMany: (misses: W[]) => Promise<ReadonlyMap<string, T>>,
): Promise<Map<string, T>> {
  const out = new Map<string, T>();
  const misses = new Map<string, W>();
  for (const w of wanted) {
    const key = keyOf(w);
    const hit = scryfallClient.getQueryData<T>([...prefix, key]);
    if (hit !== undefined) out.set(key, hit);
    else misses.set(key, w);
  }
  if (misses.size === 0) return out;
  const found = await fetchMany([...misses.values()]);
  for (const [key, value] of found) {
    scryfallClient.setQueryData([...prefix, key], value);
    out.set(key, value);
  }
  return out;
}

/** One answer per key, fetched once and then only ever read. */
export function cachedOne<T>(
  queryKey: readonly unknown[],
  queryFn: (signal: AbortSignal) => Promise<T>,
): Promise<T> {
  return scryfallClient.fetchQuery({
    queryKey,
    queryFn: ({ signal }) => queryFn(signal),
  });
}

/** IndexedDB as the persister's storage: one store, one key per client. */
function idbStorage() {
  const open = new Promise<IDBDatabase>((resolve, reject) => {
    const req = indexedDB.open("meldweb-scryfall", 1);
    req.onupgradeneeded = () => req.result.createObjectStore("cache");
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
  const run = <R>(
    mode: IDBTransactionMode,
    op: (store: IDBObjectStore) => IDBRequest<R>,
  ) =>
    open.then(
      (db) =>
        new Promise<R>((resolve, reject) => {
          const req = op(db.transaction("cache", mode).objectStore("cache"));
          req.onsuccess = () => resolve(req.result);
          req.onerror = () => reject(req.error);
        }),
    );
  return {
    getItem: (key: string) =>
      run<string | undefined>("readonly", (s) => s.get(key)).then(
        (v) => v ?? null,
      ),
    setItem: (key: string, value: string) =>
      run("readwrite", (s) => s.put(value, key)).then(() => undefined),
    removeItem: (key: string) =>
      run("readwrite", (s) => s.delete(key)).then(() => undefined),
  };
}

/** How long the cache waits, once it has changed, before writing itself out. */
export const SAVE_MS = 1000;

/**
 * Restores the last session's answers and keeps saving new ones. The page
 * calls it once at start; where there is no IndexedDB (tests, a locked-down
 * browser) the cache simply lasts as long as the tab.
 *
 * TanStack's own `persistQueryClient` dehydrates the whole cache on every
 * change and throttles only the write, so caching the thousands of printings
 * an import looks up dehydrated an ever larger cache once per card: minutes,
 * growing with every import. Here a change only schedules a save, and the
 * cache is dehydrated once per `SAVE_MS` however much changed.
 */
export async function persistScryfallCache(): Promise<void> {
  if (typeof indexedDB === "undefined") return;
  const options = {
    queryClient: scryfallClient,
    persister: createAsyncStoragePersister({
      storage: idbStorage(),
      key: "scryfall",
      // The save below is already one per SAVE_MS.
      throttleTime: 0,
    }),
    maxAge: CACHE_MS,
    // Bump when a cached shape changes, so old answers are dropped rather
    // than read as the new shape.
    buster: "3",
  };
  // A cache that cannot be read is an empty cache, not a page that fails.
  await persistQueryClientRestore(options).catch(() => undefined);
  let scheduled = false;
  scryfallClient.getQueryCache().subscribe(() => {
    if (scheduled) return;
    scheduled = true;
    setTimeout(() => {
      scheduled = false;
      void persistQueryClientSave(options).catch(() => undefined);
    }, SAVE_MS);
  });
}
