// The engine wrapper for the web host.
//
// Same bootstrap sequence and job protocol as js/engine.js (FINDINGS.md S7/S8),
// but nothing underneath is built: the page already is the browser core.js was
// written for. Workers are Web Workers, timers are timers, and the file-written
// hook takes its `window` branch. What this file adds is only the sequence -
// and the one thing the page may lack, which is a `window` (see loadCore).
//
// Rust hands in what it decides: the fingerprinted, tag-patched core.wasm, the
// ABI constants, the tier, and the job decoder. The glue never picks any of
// those itself, exactly as on the native host.

const fail = (message) => {
  throw new Error(message);
};
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const text = (u8) => new TextDecoder().decode(u8);

// Every call gets the host's deadline, as it does natively: a job the engine
// never answers is an error, not a future that never resolves.
function bounded(promise, what, ms) {
  let timer;
  const deadline = new Promise((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} timed out after ${ms} ms`)), ms);
  });
  return Promise.race([promise, deadline]).finally(() => clearTimeout(timer));
}

async function fetchBytes(url) {
  const response = await fetch(url);
  if (!response.ok) fail(`${url}: HTTP ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
}

// Every engine's JobQueue, so one file-written event reaches all of them. The
// event says only that *a* file was written, so each queue checks its own slot.
const queues = new Set();
const kickAll = () => {
  for (const q of queues) q.kick();
};

// core.js is a classic script that defines `createCore`, not a module, so it
// goes in the way classic scripts go in. Loaded once per page however many
// engines open. `url` is absolute, and is also what the pthread pool spawns
// its Web Workers from.
let coreLoaded = null;
function loadCore(url) {
  coreLoaded ??= (async () => {
    if (typeof createCore === "function") return;
    if (typeof document !== "undefined") {
      await new Promise((resolve, reject) => {
        const script = document.createElement("script");
        script.src = url;
        script.onload = resolve;
        script.onerror = () => reject(new Error(`could not load ${url}`));
        document.head.append(script);
      });
    } else {
      // A dedicated worker. A classic one has importScripts; a module worker -
      // which is what a wasm-bindgen `--target web` worker is - has the name
      // but throws, so it evaluates the fetched source at global scope, which
      // is what importScripts does anyway.
      try {
        importScripts(url);
      } catch {
        (0, eval)(`${await (await fetch(url)).text()}\n//# sourceURL=${url}`);
      }
    }
    if (typeof createCore !== "function") fail(`${url} did not define createCore`);
  })();
  return coreLoaded;
}

// The shipped EM_ASM hook (FINDINGS.md S6) is
//   window ? window.dispatchEvent(new CustomEvent("file-written"))
//          : global.fileEvents.emit("file-written")
// On a page the first branch fires and a listener on window hears it. In a
// dedicated worker there is no window, so the second branch needs its
// `global.fileEvents` - the same two-method object the native host builds.
if (typeof window !== "undefined") {
  window.addEventListener("file-written", kickAll);
} else {
  globalThis.global ??= globalThis;
  globalThis.fileEvents ??= { on() {}, emit: kickAll };
}

// Most exports return a requestId. 0 means the result is already in
// main.output; anything else appears as worker_N.output, N cycling through
// the output slots, drained in strict order and acked with _process_next.
class JobQueue {
  #mod;
  #decode;
  #slots;
  #pending = [];
  #slot = 0;
  #scheduled = false;
  #delay = 4;

  constructor(mod, decode, slots) {
    this.#mod = mod;
    this.#decode = decode;
    this.#slots = slots;
    queues.add(this);
  }

  close() {
    queues.delete(this);
  }

  kick() {
    this.#drainSoon(4);
  }

  submit(requestId, onProgress) {
    if (!requestId) {
      try {
        return Promise.resolve(this.#decode(this.#mod.FS.readFile("main.output")));
      } catch {
        return Promise.resolve(null);
      }
    }
    return new Promise((resolve, reject) => {
      this.#pending.push({ requestId, resolve, reject, onProgress });
      this.#drainSoon(4);
    });
  }

  #drainSoon(delay) {
    if (this.#scheduled) return;
    this.#scheduled = true;
    setTimeout(() => {
      this.#scheduled = false;
      this.#drain();
    }, delay);
  }

  #drain() {
    if (this.#pending.length === 0) return;
    const { FS } = this.#mod;
    try {
      for (;;) {
        const file = `worker_${this.#slot}.output`;
        if (!FS.analyzePath(file).exists) {
          this.#delay = Math.min(this.#delay * 2, 250);
          break;
        }
        const msg = this.#decode(FS.readFile(file));
        const idx = this.#pending.findIndex((p) => p.requestId === msg.id);

        // A result nobody is waiting for still occupies its slot: drop it and
        // advance without acking, or every later result is stuck behind it.
        if (idx === -1) {
          FS.unlink(file);
          this.#slot = (this.#slot + 1) % this.#slots;
          continue;
        }

        const job = this.#pending[idx];
        FS.unlink(file);
        this.#mod._process_next(msg.id);
        this.#slot = (this.#slot + 1) % this.#slots;

        if (msg.status === "running") {
          job.onProgress?.(msg);
          continue;
        }
        this.#pending.splice(idx, 1);
        if (msg.status === "error") {
          job.reject(new Error(msg.message || "job failed"));
        } else {
          job.resolve(msg);
        }
        this.#delay = 4;
      }
    } catch {
      this.#delay = Math.min(this.#delay * 2, 250);
    }
    if (this.#pending.length > 0) this.#drainSoon(this.#delay);
  }
}

/**
 * Bring up one engine. `base` is where the page serves the files
 * gitaxian-probe-assets lays out, `wasm` the core.wasm bytes the host has
 * already fingerprinted and patched, `abi` the host's constants, `decode` the
 * host's job decoder (bytes -> JSON string), and `progress` an optional
 * (stage, percent, message) callback.
 */
export function open(base, files, wasm, model, tier, abi, decode, progress, timeoutMs) {
  // Boot unpacks a 44 MB catalogue and loads a 34 MB model, so it gets more
  // room than an ordinary call - the same floor the native host uses. The
  // deadline covers createCore too: a pool that cannot start never settles it.
  return bounded(
    boot(base, files, wasm, model, tier, abi, decode, progress, timeoutMs),
    "booting the engine",
    Math.max(timeoutMs, 120000),
  );
}

// `model` is the weights, which the Rust side fetched packed and unpacked.
// `files` names where a file is when it is not at `base` + its name.
async function boot(base, files, wasm, model, tier, abi, decode, progress, timeoutMs) {
  const root = new URL(base, globalThis.location.href);
  const url = (name) =>
    Object.hasOwn(files, name)
      ? new URL(files[name], globalThis.location.href).href
      : new URL(name, root).href;
  const report = (stage, percent, message) => progress?.(stage, percent, message);
  const decodeJob = (u8) => JSON.parse(decode(u8));

  report("load", 0, "loading core.js");
  const coreUrl = url("core.js");
  const [, db7z, md5, size, version] = await Promise.all([
    loadCore(coreUrl),
    fetchBytes(url("data.7z")),
    fetchBytes(url("data.md5")),
    fetchBytes(url("data.size")),
    fetchBytes(url("version.txt")),
  ]);

  report("load", 0, "instantiating");
  const mod = await createCore({
    // This build declares `var wasmBinary` as a bare local, so
    // Module.wasmBinary is ignored; instantiateWasm is the only injection
    // point that works.
    instantiateWasm(imports, cb) {
      WebAssembly.instantiate(wasm, imports).then((r) => cb(r.instance, r.module));
      return {};
    },
    // Where the pthread pool's Web Workers load from. Set explicitly because
    // core.js otherwise infers it from document.currentScript, which a worker
    // host does not have.
    mainScriptUrlOrBlob: coreUrl,
    locateFile: (f) => url(f),
    printErr: () => {},
  });
  const probe = new Probe(mod, new JobQueue(mod, decodeJob, abi.outputSlots), abi, timeoutMs);
  return probe.boot(tier, { db7z, md5, size, version, model }, report);
}

class Probe {
  #mod;
  #jobs;
  #abi;
  #timeoutMs;
  #maskPtr = null;
  #dataPtr = null;
  #closed = false;
  version = "unknown";

  constructor(mod, jobs, abi, timeoutMs) {
    this.#mod = mod;
    this.#jobs = jobs;
    this.#abi = abi;
    this.#timeoutMs = timeoutMs;
  }

  #alive() {
    if (this.#closed) fail("engine is closed");
  }

  #bounded(promise, what) {
    return bounded(promise, what, this.#timeoutMs);
  }

  #call(name, ...args) {
    const mod = this.#mod;
    if (typeof mod[name] !== "function") fail(`engine has no export ${name}`);
    return this.#jobs.submit(mod[name](...args));
  }

  async #withString(str, fn) {
    const mod = this.#mod;
    const ptr = mod.stringToNewUTF8(str);
    try {
      return await this.#jobs.submit(fn(ptr));
    } finally {
      mod._free(ptr);
    }
  }

  #malloc(bytes, what) {
    const ptr = this.#mod._malloc(bytes.length);
    if (!ptr) fail(`could not allocate ${bytes.length} bytes for ${what}`);
    this.#mod.HEAPU8.set(bytes, ptr);
    return ptr;
  }

  async boot(tier, files, report) {
    const mod = this.#mod;
    const boot = async () => {
      for (const dir of ["/opfs", "/opfs/cache"]) {
        try {
          mod.FS.mkdir(dir);
        } catch { /* already there */ }
      }
      // _install refuses to unpack unless the version sidecars sit alongside it.
      mod.FS.writeFile("opfs/cache/data.7z", files.db7z);
      mod.FS.writeFile("opfs/cache/data.md5", files.md5);
      mod.FS.writeFile("opfs/cache/data.size", files.size);
      mod.FS.writeFile("opfs/cache/version.txt", files.version);

      report("setup", 0, "setup_system");
      await this.#call("_setup_system");

      const status = await this.#call("_installation_status");
      if (!status?.data?.installed) {
        report("install", 0, "unpacking catalogue");
        const p = mod.stringToNewUTF8("opfs/cache/data.7z");
        try {
          await this.#jobs.submit(mod._install(p), (m) =>
            report("install", m.progress ?? 0, m.message ?? ""));
        } finally {
          mod._free(p);
        }
      }

      // _initialize is the sqlite3_deserialize call and wants the UNPACKED
      // data.db bytes that _install leaves at opfs/data.db. sqlite keeps this
      // buffer as the live backing store, so it is held until close().
      report("attach", 0, "deserialising catalogue");
      const db = mod.FS.readFile("opfs/data.db");
      mod.FS.unlink("opfs/data.db");
      this.#dataPtr = this.#malloc(db, "the catalogue");
      await this.#jobs.submit(mod._initialize(this.#dataPtr, db.length));

      // Order matters: _rec_init_rec brings up the recognition subsystem and
      // clears whatever model was selected, so the model must load after it.
      report("recogniser", 0, "starting recogniser");
      await this.#startRecogniser();

      report("model", 0, `loading ${tier} model`);
      const modelPtr = this.#malloc(files.model, "the model");
      try {
        await this.#jobs.submit(mod._rec_alpha_init(modelPtr, files.model.length));
      } finally {
        mod._free(modelPtr);
      }
      await this.#call("_rec_set_model", 0);

      this.#maskPtr = mod._malloc(this.#abi.maskBytes);
      mod.HEAPU8.fill(0, this.#maskPtr, this.#maskPtr + this.#abi.maskBytes);
      this.version = text(files.version).trim() || "unknown";
      report("ready", 100, "ready");
      return this;
    };
    return boot();
  }

  // _rec_init_rec does not use the job channel: it signals completion by
  // creating recognition.output with a JSON status blob.
  async #startRecogniser() {
    const mod = this.#mod;
    mod._rec_init_rec();
    let delay = 10;
    for (;;) {
      if (mod.FS.analyzePath("recognition.output").exists) {
        const parsed = JSON.parse(text(mod.FS.readFile("recognition.output")));
        if (parsed.status === "success") return parsed;
        fail(parsed.message || "recogniser failed to start");
      }
      await sleep(delay);
      delay = Math.min(delay * 2, 250);
    }
  }

  query(sql) {
    this.#alive();
    return this.#bounded(
      this.#withString(sql, (p) => this.#mod._sql_query(p))
        .then((r) => JSON.stringify(r?.data ?? [])),
      "query",
    );
  }

  exec(sql) {
    this.#alive();
    return this.#bounded(
      this.#withString(sql, (p) => this.#mod._sql_exec(p))
        .then((r) => JSON.stringify(r?.data ?? null)),
      "exec",
    );
  }

  /** Find the card quad without identifying it - the crop detector. */
  locate(rgba, width, height) {
    this.#alive();
    const mod = this.#mod;
    const run = async () => {
      const ptr = this.#malloc(rgba, "the frame");
      try {
        const r = await this.#jobs.submit(mod._rec_best_detection(ptr, height, width));
        const corners = r?.data?.corners ?? [];
        if (corners.length !== 8) return JSON.stringify(null);
        return JSON.stringify([0, 2, 4, 6].map((i) => ({ x: corners[i], y: corners[i + 1] })));
      } finally {
        mod._free(ptr);
      }
    };
    return this.#bounded(run(), "locate");
  }

  /**
   * Identify the cards in a still image. The engine is built around a live
   * camera feed, so this pushes the frame through the streaming detector until
   * it settles.
   */
  recognize(rgba, width, height, maxFrames, settleTimeoutMs) {
    this.#alive();
    const mod = this.#mod;
    const abi = this.#abi;
    const run = async () => {
      mod._rec_clear_tracking();
      for (let frame = 0; frame < maxFrames; frame++) {
        const ptr = this.#malloc(rgba, "the frame");
        try {
          mod._rec_det_rec(ptr, height, width, frame === 0 ? 1 : 0, this.#maskPtr);
        } finally {
          mod._free(ptr);
        }

        const deadline = performance.now() + settleTimeoutMs;
        let status = abi.recRunning;
        while (performance.now() < deadline) {
          await sleep(20);
          status = mod._rec_status();
          if (status !== abi.recRunning) break;
        }
        if (status === abi.recFinishedWithDetections) {
          const cards = this.#takeDetections()?.cards;
          if (cards?.length) return JSON.stringify(cards);
        }
      }
      return JSON.stringify([]);
    };
    return this.#bounded(run(), "recognize");
  }

  #takeDetections() {
    const mod = this.#mod;
    try {
      const raw = mod.FS.readFile("recognition.json");
      mod.FS.unlink("recognition.json");
      return JSON.parse(text(raw));
    } catch {
      return null;
    }
  }

  /**
   * Release the engine's buffers. The 32 pthread workers are not reachable:
   * nothing on Module exposes PThread (FINDINGS.md S6), so they live as long
   * as the page, as they do in Delver's own app.
   */
  close() {
    if (this.#closed) return;
    this.#closed = true;
    this.#jobs.close();
    for (const ptr of [this.#maskPtr, this.#dataPtr]) {
      if (ptr) {
        try {
          this.#mod._free(ptr);
        } catch { /* tearing down anyway */ }
      }
    }
    this.#maskPtr = null;
    this.#dataPtr = null;
  }
}

export { fetchBytes };
