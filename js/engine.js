"use strict";
// The engine wrapper, running inside the sandbox.
//
// Same bootstrap sequence and job protocol as the Node harness (FINDINGS.md
// S7/S8) - the difference is what is underneath: artefact bytes arrive through
// a host op instead of `fs`, and every method hands its result back to Rust as
// JSON rather than as a live object.
((globalThis) => {
  const core = Deno.core;
  const ops = core.ops;
  const decodeMsgpack = globalThis.__delverDecodeMsgpack;

  // Engine ABI constants and the per-tier init export/id are the host's to
  // decide (src/lib.rs); this glue only carries them, never defines them. The
  // tier -> init -> token mapping lives in Rust on purpose - see Model there for
  // why keeping it out of here is what keeps the JWT gate from being a one-line
  // edit in the sandbox.
  const ABI = JSON.parse(ops.op_delver_abi());

  const fail = (message) => {
    throw new Error(message);
  };
  const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
  const text = (u8) => core.decode(u8);

  // Most exports return a requestId. 0 means the result is already in
  // main.output; anything else appears as worker_N.output, N cycling 0..99,
  // drained in strict order and acked with _process_next.
  class JobQueue {
    #mod;
    #pending = [];
    #slot = 0;
    #scheduled = false;
    #delay = 4;

    constructor(mod) {
      this.#mod = mod;
      globalThis.fileEvents.on("file-written", () => this.#drainSoon(4));
    }

    submit(requestId, onProgress) {
      if (!requestId) {
        try {
          return Promise.resolve(decodeMsgpack(this.#mod.FS.readFile("main.output")));
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
          const msg = decodeMsgpack(FS.readFile(file));
          const idx = this.#pending.findIndex((p) => p.requestId === msg.id);

          // A result nobody is waiting for still occupies its slot: drop it and
          // advance without acking, or every later result is stuck behind it.
          if (idx === -1) {
            FS.unlink(file);
            this.#slot = (this.#slot + 1) % ABI.outputSlots;
            continue;
          }

          const job = this.#pending[idx];
          FS.unlink(file);
          this.#mod._process_next(msg.id);
          this.#slot = (this.#slot + 1) % ABI.outputSlots;

          if (msg.status === "running") {
            job.onProgress?.(msg);
            continue;
          }
          this.#pending.splice(idx, 1);
          if (msg.status === "error") {
            const err = new Error(msg.message || "job failed");
            err.jobResult = msg;
            job.reject(err);
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

  let mod = null;
  let jobs = null;
  let maskPtr = null;
  let dataPtr = null;
  let closed = false;

  const alive = () => {
    if (closed) fail("engine is closed");
    if (!mod) fail("engine is not open");
  };
  const progress = (stage, percent, message) =>
    ops.op_delver_progress(stage, percent, message);

  const call = (name, ...args) => {
    alive();
    if (typeof mod[name] !== "function") fail(`engine has no export ${name}`);
    return jobs.submit(mod[name](...args));
  };

  const withBuffer = async (bytes, fn) => {
    const ptr = mod._malloc(bytes.length);
    if (!ptr) fail(`could not allocate ${bytes.length} bytes in the wasm heap`);
    try {
      mod.HEAPU8.set(bytes, ptr);
      return await jobs.submit(fn(ptr, bytes.length));
    } finally {
      mod._free(ptr);
    }
  };

  const withString = async (str, fn) => {
    const ptr = mod.stringToNewUTF8(str);
    try {
      return await jobs.submit(fn(ptr));
    } finally {
      mod._free(ptr);
    }
  };

  // _rec_init_rec does not use the job channel: it signals completion by
  // creating recognition.output with a JSON status blob.
  const startRecogniser = async (timeoutMs = 60000) => {
    mod._rec_init_rec();
    const deadline = ops.op_delver_now() + timeoutMs;
    let delay = 10;
    for (;;) {
      if (mod.FS.analyzePath("recognition.output").exists) {
        const parsed = JSON.parse(text(mod.FS.readFile("recognition.output")));
        if (parsed.status === "success") return parsed;
        fail(parsed.message || "recogniser failed to start");
      }
      if (ops.op_delver_now() > deadline) fail("recogniser did not start within timeout");
      await sleep(delay);
      delay = Math.min(delay * 2, 250);
    }
  };

  const assertImage = (width, height, data) => {
    const want = width * height * 4;
    if (data.length !== want) {
      fail(`expected ${want} bytes of RGBA for ${width}x${height}, got ${data.length}`);
    }
  };

  const pushFrame = (data, width, height, firstFrame) => {
    const ptr = mod._malloc(data.length);
    if (!ptr) fail(`could not allocate ${data.length} bytes in the wasm heap`);
    try {
      mod.HEAPU8.set(data, ptr);
      mod._rec_det_rec(ptr, height, width, firstFrame ? 1 : 0, maskPtr);
    } finally {
      mod._free(ptr);
    }
  };

  const takeDetections = () => {
    try {
      const raw = mod.FS.readFile("recognition.json");
      mod.FS.unlink("recognition.json");
      return JSON.parse(text(raw));
    } catch {
      return null;
    }
  };

  globalThis.__delver = {
    // `init` and `modelId` are resolved host-side (Model in src/lib.rs), which
    // is also where the token requirement was already enforced; this trusts
    // both rather than re-deriving them.
    async boot({ model }) {
      if (mod) fail("engine is already open");

      progress("load", 0, "instantiating");
      // The host has already fingerprinted core.wasm and exported its two
      // internal tags; these are the patched bytes.
      const wasm = ops.op_delver_wasm();
      mod = await createCore({
        // This build declares `var wasmBinary` as a bare local, so
        // Module.wasmBinary is ignored; instantiateWasm is the only injection
        // point that works.
        instantiateWasm(imports, cb) {
          WebAssembly.instantiate(wasm, imports).then((r) => cb(r.instance, r.module));
        },
        mainScriptUrlOrBlob: "core.js",
        locateFile: (f) => f,
        printErr: () => {},
      });
      jobs = new JobQueue(mod);

      for (const dir of ["/opfs", "/opfs/cache"]) {
        try {
          mod.FS.mkdir(dir);
        } catch { /* already there */ }
      }

      // _install refuses to unpack unless the version sidecars sit alongside it.
      mod.FS.writeFile("opfs/cache/data.7z", ops.op_delver_artifact("data.7z"));
      for (const [dst, src] of [
        ["opfs/cache/data.md5", "data.md5"],
        ["opfs/cache/data.size", "data.size"],
        ["opfs/cache/version.txt", "version.txt"],
      ]) {
        const bytes = ops.op_delver_artifact(src);
        if (bytes.length) mod.FS.writeFile(dst, bytes);
      }

      progress("setup", 0, "setup_system");
      await call("_setup_system");

      const status = await call("_installation_status");
      if (!status?.data?.installed) {
        progress("install", 0, "unpacking catalogue");
        const p = mod.stringToNewUTF8("opfs/cache/data.7z");
        try {
          await jobs.submit(mod._install(p), (m) =>
            progress("install", m.progress ?? 0, m.message ?? ""));
        } finally {
          mod._free(p);
        }
      }

      // _initialize is the sqlite3_deserialize call and wants the UNPACKED
      // data.db bytes that _install leaves at opfs/data.db. Handing it the .7z
      // instead succeeds while leaving the `data` schema empty.
      progress("attach", 0, "deserialising catalogue");
      const db = mod.FS.readFile("opfs/data.db");
      mod.FS.unlink("opfs/data.db");
      // sqlite keeps this buffer as the live backing store for the catalogue.
      // Freeing it turns every later query into "file is not a database", so it
      // is held for the lifetime of the engine, as the app does.
      dataPtr = mod._malloc(db.length);
      if (!dataPtr) fail(`could not allocate ${db.length} bytes for the catalogue`);
      mod.HEAPU8.set(db, dataPtr);
      await jobs.submit(mod._initialize(dataPtr, db.length));

      // Order matters: _rec_init_rec brings up the recognition subsystem and
      // clears whatever model was selected, so the model must load after it.
      progress("recogniser", 0, "starting recogniser");
      await startRecogniser();

      progress("model", 0, `loading ${model} model`);
      const modelBytes = ops.op_delver_artifact(`model-${model}.dat`);
      await withBuffer(modelBytes, (ptr, len) => mod._rec_alpha_init(ptr, len));
      await call("_rec_set_model", 0);

      maskPtr = mod._malloc(ABI.maskBytes);
      mod.HEAPU8.fill(0, maskPtr, maskPtr + ABI.maskBytes);
      progress("ready", 100, "ready");

      return JSON.stringify({
        version: text(ops.op_delver_artifact("version.txt")).trim() || "unknown",
        model,
      });
    },

    async query(sql) {
      alive();
      const r = await withString(sql, (p) => mod._sql_query(p));
      return JSON.stringify(r?.data ?? []);
    },

    async exec(sql) {
      alive();
      const r = await withString(sql, (p) => mod._sql_exec(p));
      return JSON.stringify(r?.data ?? null);
    },

    async installationStatus() {
      return JSON.stringify((await call("_installation_status"))?.data ?? null);
    },

    async systemStats() {
      return JSON.stringify((await call("_system_stats_v2"))?.data ?? null);
    },

    /** Find the card quad without identifying it - the crop detector. */
    async locate(width, height) {
      alive();
      const data = ops.op_delver_take_image();
      assertImage(width, height, data);
      const ptr = mod._malloc(data.length);
      if (!ptr) fail(`could not allocate ${data.length} bytes in the wasm heap`);
      try {
        mod.HEAPU8.set(data, ptr);
        const r = await jobs.submit(mod._rec_best_detection(ptr, height, width));
        const corners = r?.data?.corners ?? [];
        if (corners.length !== 8) return JSON.stringify(null);
        return JSON.stringify(
          [0, 2, 4, 6].map((i) => ({ x: corners[i], y: corners[i + 1] })),
        );
      } finally {
        mod._free(ptr);
      }
    },

    /**
     * Identify the cards in a still image. The engine is built around a live
     * camera feed, so this pushes the frame through the streaming detector
     * until it settles. It needs to see the whole card against a contrasting
     * background; a tight edge-to-edge crop gives it no boundary to find.
     */
    async recognize(width, height, maxFrames, settleTimeoutMs) {
      alive();
      const data = ops.op_delver_take_image();
      assertImage(width, height, data);
      mod._rec_clear_tracking();

      for (let frame = 0; frame < maxFrames; frame++) {
        pushFrame(data, width, height, frame === 0);

        const deadline = ops.op_delver_now() + settleTimeoutMs;
        let status = ABI.recRunning;
        while (ops.op_delver_now() < deadline) {
          await sleep(20);
          status = mod._rec_status();
          if (status !== ABI.recRunning) break;
        }
        if (status === ABI.recFinishedWithDetections) {
          const detections = takeDetections();
          if (detections?.cards?.length) return JSON.stringify(detections.cards);
        }
      }
      return JSON.stringify([]);
    },

    close() {
      if (closed) return;
      closed = true;
      for (const ptr of [maskPtr, dataPtr]) {
        if (ptr) {
          try {
            mod._free(ptr);
          } catch { /* tearing down anyway */ }
        }
      }
      maskPtr = null;
      dataPtr = null;
    },
  };
})(globalThis);
