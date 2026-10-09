/**
 * The page's copy of Scryfall (ADR-0030), in a worker of its own: it holds
 * the copy in memory for as long as the tab is open and answers lookups off
 * the main thread.
 *
 * The copy lives in origin-private storage, gzipped, in one of two slots, and
 * a small meta file written last names the slot holding a whole copy. A new
 * copy goes in the other slot, so the one the meta names is never touched.
 * The first visit makes it from Scryfall's bulk files, streamed through the
 * wasm a few megabytes at a time; a later visit reads it back, and once a day
 * asks Scryfall whether a newer file is out, making a new copy in the
 * background while the old one keeps answering.
 */
import init, {
  copy_autocomplete,
  copy_begin,
  copy_feed_cards,
  copy_feed_tags,
  copy_finish,
  copy_format,
  copy_load,
  copy_lookup,
  copy_prints,
  copy_search,
  copy_warm,
} from "../wasm/pkg/meldweb_wasm.js";
import type { Asked, CopyState, Request, Told } from "./protocol";

const SLOTS = ["scryfall-copy.a.txt.gz", "scryfall-copy.b.txt.gz"] as const;
type Slot = (typeof SLOTS)[number];
const META = "scryfall-copy.meta.json";
/**
 * Scryfall's `updated_at` for a Default Cards file the wasm could not make a
 * copy of. Asking again would download the same 85 MB to fail the same way,
 * so the API answers until Scryfall writes a new one.
 */
const UNREADABLE = "scryfall-copy.unreadable.txt";
/** One copy is made at a time across every tab of the site. */
const LOCK = "scryfall-copy";
/** How often an open tab looks whether its copy is a day old. */
const LOOK_MS = 60 * 60 * 1000;
/**
 * Scryfall rewrites its bulk files every 12 to 24 hours, and with them the
 * day's prices, so a copy a day old is asked about once.
 */
const REFRESH_MS = 24 * 60 * 60 * 1000;
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

interface Meta {
  format: number;
  slot: Slot;
  /** Scryfall's `updated_at` for the Default Cards file the copy was made from. */
  updatedAt: string;
  /** When this page last made the copy or found it current. */
  checkedAt: number;
}

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

async function readMeta(dir: FileSystemDirectoryHandle): Promise<Meta | null> {
  try {
    const meta = JSON.parse(await (await readText(dir, META)).text()) as Meta;
    return meta.format === copy_format() && SLOTS.includes(meta.slot)
      ? meta
      : null;
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
  meta: Meta,
): Promise<string> {
  const file = await readText(dir, meta.slot);
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
 * Makes a new copy from Scryfall's files, which answers from then on, and
 * keeps it in the slot `meta`, what storage holds, does not name.
 */
async function make(
  dir: FileSystemDirectoryHandle,
  meta: Meta | null,
  files: { cards: BulkFile; tags: BulkFile },
  progress: (received: number, total: number) => void,
): Promise<Meta> {
  const total = files.cards.compressed_size;
  let told = 0;
  wasm(() => copy_begin(files.cards.updated_at));
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
  const made: Meta = {
    format: copy_format(),
    slot: meta?.slot === SLOTS[0] ? SLOTS[1] : SLOTS[0],
    updatedAt: files.cards.updated_at,
    checkedAt: Date.now(),
  };
  if (state.kind !== "ready") tell({ kind: "saving" });
  try {
    await writeStream(
      dir,
      made.slot,
      new Blob([text])
        .stream()
        .pipeThrough(
          new CompressionStream("gzip"),
        ) as ReadableStream<Uint8Array>,
    );
    await writeText(dir, META, JSON.stringify(made));
  } catch (e) {
    // The copy in memory is whole and answers this tab; the next visit
    // makes it again.
    console.warn("the copy of Scryfall could not be kept", e);
  }
  return made;
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
 * Scryfall's `updated_at` for a file this worker could not make a copy of,
 * remembered here too in case storage could not keep it.
 */
let unreadable: string | null = null;

/**
 * Whether Scryfall wrote a newer file than `had`, the copy this tab answers
 * from, making a copy of it if so. Run under the lock, so a second tab finds
 * the copy the first made rather than making its own.
 *
 * `had` is this tab's, not storage's: when storage could not keep a copy the
 * tab still answers from it, and storage's older one is not opened over it.
 */
async function refresh(
  dir: FileSystemDirectoryHandle,
  had: Meta | null,
): Promise<Meta | null> {
  const kept = await readMeta(dir);
  // ISO 8601 times written by one server sort as text.
  if (kept && kept.updatedAt > (had?.updatedAt ?? "")) {
    // Another tab made a newer copy, perhaps while this one waited.
    try {
      ready(await open(dir, kept));
      had = kept;
    } catch {}
  }
  if (had && Date.now() - had.checkedAt < REFRESH_MS) return had;
  const files = await bulkFiles();
  if (had && files.cards.updated_at === had.updatedAt) {
    const checkedAt = Date.now();
    if (kept?.updatedAt === had.updatedAt)
      await writeText(dir, META, JSON.stringify({ ...kept, checkedAt })).catch(
        () => {},
      );
    return { ...had, checkedAt };
  }
  unreadable ??= await readText(dir, UNREADABLE)
    .then((f) => f.text())
    .catch(() => null);
  if (unreadable === files.cards.updated_at)
    throw new Error(
      `this copy's reader cannot read Scryfall's file of ${unreadable}`,
    );
  if (!had && state.kind !== "ready")
    tell({ kind: "downloading", received: 0, total: 0 });
  try {
    const made = await make(dir, kept, files, (received, total) =>
      tell(
        state.kind === "ready"
          ? { ...state, refresh: { received, total } }
          : { kind: "downloading", received, total },
      ),
    );
    ready(made.updatedAt);
    return made;
  } catch (e) {
    if (e instanceof Unreadable) {
      unreadable = files.cards.updated_at;
      await writeText(dir, UNREADABLE, unreadable).catch(() => {});
    }
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
  let meta = await readMeta(dir);
  if (meta) {
    try {
      ready(await open(dir, meta));
    } catch {
      meta = null;
    }
  }
  // An open tab looks again every hour, so one left open for days still
  // has the day's prices.
  // `meta` is set inside the lock, so a look queued behind this one starts
  // from the copy this one left; and a look still downloading on a slow link
  // an hour later is not joined by a second feeding the same builder.
  let looking = false;
  const look = async () => {
    if (looking || trapped) return;
    looking = true;
    try {
      await locked(async () => {
        meta = await refresh(dir, meta);
      });
    } catch (e) {
      failed(e);
    } finally {
      looking = false;
    }
  };
  setInterval(look, LOOK_MS);
  await look();
}

main().catch(failed);
