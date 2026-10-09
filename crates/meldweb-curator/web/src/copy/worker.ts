/**
 * The page's copy of Scryfall (ADR-0030), in a worker of its own: it holds
 * the copy in memory for as long as the tab is open and answers lookups off
 * the main thread.
 *
 * The copy lives in origin-private storage as gzipped JSON beside a small
 * meta file, which is written last and so only exists beside a whole copy.
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
} from "../wasm/pkg/meldweb_wasm.js";
import type { Asked, CopyState, Request, Told } from "./protocol";

const FILE = "scryfall-copy.json.gz";
const META = "scryfall-copy.meta.json";
/**
 * Scryfall rewrites its bulk files every 12 to 24 hours, and with them the
 * day's prices, so a copy a day old is asked about once.
 */
const REFRESH_MS = 24 * 60 * 60 * 1000;
/** How much text the wasm is handed at once: big enough to amortise a call. */
const CHUNK = 4_000_000;
/** How often progress is told, at most. */
const PROGRESS_MS = 250;

interface Meta {
  format: number;
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

function answer(request: Request): string {
  switch (request.kind) {
    case "lookup":
      return copy_lookup(JSON.stringify(request.wanted));
    case "prints":
      return copy_prints(request.uri);
    case "search":
      return copy_search(request.query, request.offset, request.limit);
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
  }
};

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
    return meta.format === copy_format() ? meta : null;
  } catch {
    return null;
  }
}

function writeMeta(dir: FileSystemDirectoryHandle, meta: Meta) {
  return writeStream(
    dir,
    META,
    new Blob([JSON.stringify(meta)]).stream() as ReadableStream<Uint8Array>,
  );
}

async function open(dir: FileSystemDirectoryHandle): Promise<string> {
  const file = await readText(dir, FILE);
  const text = await new Response(
    file.stream().pipeThrough(new DecompressionStream("gzip")),
  ).text();
  return copy_load(text);
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

/** Makes a new copy from Scryfall's files and keeps it; it answers from then on. */
async function make(
  dir: FileSystemDirectoryHandle,
  files: { cards: BulkFile; tags: BulkFile },
  progress: (received: number, total: number) => void,
): Promise<string> {
  const total = files.cards.compressed_size;
  let told = 0;
  copy_begin(files.cards.updated_at);
  await stream(files.cards.jsonl_download_uri, copy_feed_cards, (received) => {
    const now = Date.now();
    if (now - told < PROGRESS_MS) return;
    told = now;
    progress(received, total);
  });
  await stream(files.tags.jsonl_download_uri, copy_feed_tags, () => {});
  const text = copy_finish();
  if (state.kind !== "ready") tell({ kind: "saving" });
  // The meta goes first and comes back last, so a copy cut short by a
  // closed tab is never read as whole.
  await dir.removeEntry(META).catch(() => {});
  await writeStream(
    dir,
    FILE,
    new Blob([text])
      .stream()
      .pipeThrough(new CompressionStream("gzip")) as ReadableStream<Uint8Array>,
  );
  await writeMeta(dir, {
    format: copy_format(),
    updatedAt: files.cards.updated_at,
    checkedAt: Date.now(),
  });
  return files.cards.updated_at;
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
      tell({ kind: "ready", updatedAt: await open(dir) });
    } catch {
      meta = null;
    }
  }
  if (meta && Date.now() - meta.checkedAt < REFRESH_MS) return;
  if (!meta) tell({ kind: "downloading", received: 0, total: 0 });
  let files: Awaited<ReturnType<typeof bulkFiles>>;
  try {
    files = await bulkFiles();
  } catch (e) {
    if (state.kind !== "ready") throw e;
    return;
  }
  if (meta && files.cards.updated_at === meta.updatedAt) {
    await writeMeta(dir, { ...meta, checkedAt: Date.now() });
    return;
  }
  const updatedAt = await make(dir, files, (received, total) =>
    tell(
      state.kind === "ready"
        ? { ...state, refresh: { received, total } }
        : { kind: "downloading", received, total },
    ),
  );
  tell({ kind: "ready", updatedAt });
}

main().catch((e: unknown) => {
  // A copy already answering keeps answering when its refresh fails.
  if (state.kind === "ready") {
    const { refresh: _, ...ready } = state;
    tell(ready);
    return;
  }
  tell({
    kind: "failed",
    message: e instanceof Error ? e.message : String(e),
  });
});
