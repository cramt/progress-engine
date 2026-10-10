/**
 * The page's copy of Scryfall (ADR-0030), in a worker of its own: it holds
 * the copy in memory for as long as the tab is open and answers lookups off
 * the main thread.
 *
 * The copy lives in origin-private storage, gzipped, in one of two slots, and
 * a small meta file written last names the slot holding a whole copy. The
 * first visit makes it from Scryfall's bulk files, streamed through the wasm
 * a few megabytes at a time; a later visit reads it back, and once a day asks
 * Scryfall whether a newer file is out, making a new copy in the background
 * while the old one keeps answering.
 *
 * Which slot, whether a copy is a day old and which meta to write are
 * `chip_scryfall::copy::store`'s, through the wasm's `store_*` functions,
 * which Gauntlet's own copy shares. This worker reads, writes and downloads
 * what they name.
 */

import type { StoreStep } from "../deck.gen";
import init, {
  copy_autocomplete,
  copy_feed_cards,
  copy_feed_tags,
  copy_finish,
  copy_load,
  copy_lookup,
  copy_prints,
  copy_search,
  copy_warm,
  store_built,
  store_due,
  store_hold,
  store_look_ms,
  store_meta_file,
  store_newer,
  store_plan,
  store_unreadable,
  store_unreadable_file,
} from "../wasm/pkg/meldweb_wasm.js";
import type { Asked, CopyState, Request, Told } from "./protocol";

/** One copy is made at a time across every tab of the site. */
const LOCK = "scryfall-copy";
/** How much text the wasm is handed at once: big enough to amortise a call. */
const CHUNK = 4_000_000;
/** How often progress is told, at most. */
const PROGRESS_MS = 250;
/**
 * How many cards or printings the copy reads for search between two of the
 * page's questions. 500 took up to 214 ms in wasm under Node; 200 keeps a
 * lookup asked meanwhile waiting under a tenth of a second.
 */
const WARM_SLICE = 200;

interface BulkFile {
  type: string;
  updated_at: string;
  jsonl_download_uri: string;
  compressed_size: number;
}

// The handle a worker may write a file through. TypeScript's DOM library
// leaves it out, since a window cannot have one.
interface SyncAccess {
  write(data: Uint8Array, at: { at: number }): number;
  truncate(size: number): void;
  flush(): void;
  close(): void;
}
type WritableFile = FileSystemFileHandle & {
  createSyncAccessHandle(): Promise<SyncAccess>;
};

const scope = globalThis as unknown as {
  postMessage(message: Told): void;
  onmessage: ((e: MessageEvent<Asked>) => void) | null;
};

let state: CopyState = { kind: "opening" };
function tell(next: CopyState): void {
  state = next;
  scope.postMessage({ kind: "state", state });
}

/**
 * Whether the copy has read every card for search. Until it has, a search
 * would read what is left itself, seconds with every lookup queued behind it,
 * so it is refused and the page asks Scryfall.
 */
let warmed = false;

function answer(request: Request): string {
  switch (request.kind) {
    case "lookup":
      return copy_lookup(JSON.stringify(request.wanted));
    case "prints":
      return copy_prints(request.uri);
    case "search":
      return warmed
        ? copy_search(request.query, request.offset, request.limit)
        : JSON.stringify({
            kind: "refused",
            message: "the copy is still reading itself for search",
          });
    case "autocomplete":
      return copy_autocomplete(request.query);
  }
}

scope.onmessage = ({ data: { id, request } }) => {
  try {
    scope.postMessage({ kind: "answer", id, json: answer(request) });
  } catch (e) {
    const message = e instanceof Error ? e.message : String(e);
    scope.postMessage({ kind: "error", id, message });
    if (e instanceof WebAssembly.RuntimeError) trap(e);
  }
};

/**
 * Set by a trap (out of memory, a panic), which leaves the wasm's state
 * half-changed: it answers nothing more, no copy is made in it, and the page
 * goes to the API for good.
 */
let trapped = false;

function trap(e: WebAssembly.RuntimeError): Error {
  trapped = true;
  tell({ kind: "failed", message: e.message });
  return e;
}

/**
 * Tells the page the worker is about to spend seconds on `call`, so it asks
 * the API meanwhile, then that it answers again. The message goes out before
 * `call` blocks the worker, so the page hears in time.
 */
function busy<T>(call: () => T): T {
  if (state.kind === "ready") tell({ ...state, busy: true });
  try {
    return call();
  } finally {
    if (state.kind === "ready") {
      const { busy: _, ...answering } = state;
      tell(answering);
    }
  }
}

async function readText(dir: FileSystemDirectoryHandle, name: string) {
  const file = await (await dir.getFileHandle(name)).getFile();
  return file;
}

async function writeStream(
  dir: FileSystemDirectoryHandle,
  name: string,
  stream: ReadableStream<Uint8Array>,
): Promise<void> {
  const handle = (await dir.getFileHandle(name, {
    create: true,
  })) as WritableFile;
  const access = await handle.createSyncAccessHandle();
  try {
    access.truncate(0);
    let at = 0;
    const reader = stream.getReader();
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      at += access.write(value, { at });
    }
    access.flush();
  } finally {
    access.close();
  }
}

/** A small file's text, or null when storage holds none. */
async function readOptional(
  dir: FileSystemDirectoryHandle,
  name: string,
): Promise<string | null> {
  try {
    return await (await readText(dir, name)).text();
  } catch {
    return null;
  }
}

function writeText(dir: FileSystemDirectoryHandle, name: string, text: string) {
  return writeStream(
    dir,
    name,
    new Blob([text]).stream() as ReadableStream<Uint8Array>,
  );
}

async function open(
  dir: FileSystemDirectoryHandle,
  slot: string,
): Promise<string> {
  const file = await readText(dir, slot);
  const text = await new Response(
    file.stream().pipeThrough(new DecompressionStream("gzip")),
  ).text();
  const updatedAt = busy(() => wasm(() => copy_load(text)));
  warmed = false;
  return updatedAt;
}

async function bulkFiles(): Promise<{ cards: BulkFile; tags: BulkFile }> {
  const response = await fetch("https://api.scryfall.com/bulk-data", {
    headers: { Accept: "application/json" },
  });
  if (!response.ok)
    throw new Error(`Scryfall's bulk data list answered ${response.status}`);
  const files = ((await response.json()) as { data: BulkFile[] }).data;
  const cards = files.find((f) => f.type === "default_cards");
  const tags = files.find((f) => f.type === "oracle_tags");
  if (!cards || !tags)
    throw new Error("Scryfall's bulk data list has no Default Cards or tags");
  return { cards, tags };
}

/**
 * A gzipped JSON-lines file, fed to `feed` in whole lines, with how many
 * compressed bytes have arrived told to `progress` as they do.
 */
async function stream(
  url: string,
  feed: (lines: string) => void,
  progress: (received: number) => void,
): Promise<void> {
  const response = await fetch(url);
  if (!response.ok || !response.body)
    throw new Error(`Scryfall's bulk file answered ${response.status}`);
  let received = 0;
  const text = response.body
    .pipeThrough(
      new TransformStream<Uint8Array<ArrayBuffer>, Uint8Array<ArrayBuffer>>({
        transform(chunk, out) {
          received += chunk.byteLength;
          progress(received);
          out.enqueue(chunk);
        },
      }),
    )
    .pipeThrough(new DecompressionStream("gzip"))
    .pipeThrough(new TextDecoderStream());
  const reader = text.getReader();
  let pending = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) break;
    pending += value;
    if (pending.length >= CHUNK) {
      const cut = pending.lastIndexOf("\n");
      feed(pending.slice(0, cut));
      pending = pending.slice(cut + 1);
    }
  }
  feed(pending);
}

/** The wasm refusing Scryfall's file, which a second try would refuse too. */
class Unreadable extends Error {}

function wasm<T>(call: () => T): T {
  try {
    return call();
  } catch (e) {
    // A trap is the wasm failing, not Scryfall's file.
    if (e instanceof WebAssembly.RuntimeError) throw trap(e);
    throw new Unreadable(e instanceof Error ? e.message : String(e));
  }
}

/**
 * Makes the copy `store_plan` began from Scryfall's files, which answers from
 * then on, and keeps it in `slot`, then the meta naming it.
 */
async function make(
  dir: FileSystemDirectoryHandle,
  slot: string,
  files: { cards: BulkFile; tags: BulkFile },
  progress: (received: number, total: number) => void,
): Promise<void> {
  const total = files.cards.compressed_size;
  let told = 0;
  await stream(
    files.cards.jsonl_download_uri,
    (lines) => wasm(() => copy_feed_cards(lines)),
    (received) => {
      const now = Date.now();
      if (now - told < PROGRESS_MS) return;
      told = now;
      progress(received, total);
    },
  );
  await stream(
    files.tags.jsonl_download_uri,
    (lines) => wasm(() => copy_feed_tags(lines)),
    () => {},
  );
  const text = busy(() => wasm(() => copy_finish()));
  warmed = false;
  const meta = store_built(Date.now());
  if (state.kind !== "ready") tell({ kind: "saving" });
  try {
    await writeStream(
      dir,
      slot,
      new Blob([text])
        .stream()
        .pipeThrough(
          new CompressionStream("gzip"),
        ) as ReadableStream<Uint8Array>,
    );
    await writeText(dir, store_meta_file(), meta);
  } catch (e) {
    // The copy in memory is whole and answers this tab; the next visit
    // makes it again.
    console.warn("the copy of Scryfall could not be kept", e);
  }
}

/**
 * Reads the rest of the copy for search a slice at a time, yielding between
 * slices so lookups are answered meanwhile; a search before it is done reads
 * what is left itself.
 */
function warm(): void {
  try {
    if (copy_warm(WARM_SLICE)) warmed = true;
    else setTimeout(warm, 0);
  } catch (e) {
    if (e instanceof WebAssembly.RuntimeError) trap(e);
    console.warn("the copy of Scryfall could not read itself for search", e);
  }
}

function ready(updatedAt: string): void {
  tell({ kind: "ready", updatedAt });
  setTimeout(warm, 0);
}

/**
 * Opens the copy `meta`, storage's meta file, names, if it is newer than the
 * one this tab answers from, which another tab made. Storage's older copy is
 * not opened over this tab's, which storage may have failed to keep.
 */
async function openNewer(
  dir: FileSystemDirectoryHandle,
  meta: string | null,
): Promise<void> {
  const slot = store_newer(meta);
  if (slot === undefined || meta === null) return;
  ready(await open(dir, slot));
  store_hold(meta);
}

/**
 * Whether Scryfall wrote a newer file than the copy this tab answers from,
 * making a copy of it if so. Run under the lock, so a second tab finds the
 * copy the first made rather than making its own.
 */
async function refresh(dir: FileSystemDirectoryHandle): Promise<void> {
  const meta = await readOptional(dir, store_meta_file());
  // Another tab made a newer copy, perhaps while this one waited.
  await openNewer(dir, meta).catch(() => {});
  if (!store_due(Date.now())) return;
  const files = await bulkFiles();
  const step = JSON.parse(
    store_plan(
      meta,
      files.cards.updated_at,
      await readOptional(dir, store_unreadable_file()),
      Date.now(),
    ),
  ) as StoreStep;
  if (step.kind === "current") {
    if (step.meta !== undefined)
      await writeText(dir, store_meta_file(), step.meta).catch(() => {});
    return;
  }
  if (step.kind === "refused") throw new Error(step.message);
  if (state.kind !== "ready")
    tell({ kind: "downloading", received: 0, total: 0 });
  try {
    await make(dir, step.file, files, (received, total) =>
      tell(
        state.kind === "ready"
          ? { ...state, refresh: { received, total } }
          : { kind: "downloading", received, total },
      ),
    );
    ready(files.cards.updated_at);
  } catch (e) {
    if (e instanceof Unreadable)
      await writeText(dir, store_unreadable_file(), store_unreadable()).catch(
        () => {},
      );
    throw e;
  }
}

/** `run` while no other tab of the site is making a copy. */
function locked<T>(run: () => Promise<T>): Promise<T> {
  return navigator.locks ? navigator.locks.request(LOCK, run) : run();
}

/** A refresh that fails leaves a copy that answers answering. */
function failed(e: unknown): void {
  if (trapped) return;
  if (state.kind === "ready") {
    const { refresh: _, ...ready } = state;
    tell(ready);
    console.warn("the copy of Scryfall could not be refreshed", e);
    return;
  }
  tell({
    kind: "failed",
    message: e instanceof Error ? e.message : String(e),
  });
}

async function main(): Promise<void> {
  if (!navigator.storage?.getDirectory) {
    tell({ kind: "unavailable", reason: "no origin-private storage" });
    return;
  }
  await init();
  const dir = await navigator.storage.getDirectory();
  // Outside the lock: another tab may hold it for minutes making a copy, and
  // the one kept answers meanwhile.
  await openNewer(dir, await readOptional(dir, store_meta_file())).catch(
    () => {},
  );
  // An open tab looks again every hour, so one left open for days still
  // has the day's prices. The wasm holds which copy answers, so a look
  // queued behind this one starts from the copy this one left; and a look
  // still downloading on a slow link an hour later is not joined by a second
  // feeding the same builder.
  let looking = false;
  const look = async () => {
    if (looking || trapped) return;
    looking = true;
    try {
      await locked(() => refresh(dir));
    } catch (e) {
      failed(e);
    } finally {
      looking = false;
    }
  };
  setInterval(look, store_look_ms());
  await look();
}

main().catch(failed);
