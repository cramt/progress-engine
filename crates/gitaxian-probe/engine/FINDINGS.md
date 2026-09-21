# Delver X engine — reverse engineering notes

Investigation date: 2026-09-20/21. Target: `https://mtg.delver.app`, version `1.76.beta`.

Everything below was observed directly unless marked **[inference]**.

Status: the engine runs headless end to end — catalogue queries and card recognition
both work. `src/` is a Rust library that runs the blob inside a deno_core sandbox
(§13); see the README. The Node harness this was worked out through has since been
removed — where the notes below compare against it, that is the historical record of
how a behaviour was established, not a second codebase to go and read.

---

## 1. What ships

| File | Download | Unpacked | Notes |
|---|---|---|---|
| `core.wasm` | 8.5 MB | — | the engine |
| `core.js` | 122 KB | — | Emscripten glue, **served standalone** |
| `data.7z` | 9.6 MB | 44.4 MB (`data.db`) | SQLite catalogue |
| `model-alpha.7z` | 23.2 MB | 34.0 MB (`model-alpha.dat`) | no token |
| `model-lambda.7z` | 28.1 MB | 58.8 MB | token-gated |
| `model-gamma.7z` | 39.5 MB | 103.7 MB | token-gated |

Sidecars per blob: `<name>.md5`, `<name>.size`. Plus `/version.txt` (`1.76.beta`).

`<name>.size` is the size of the **unpacked** file and checks out exactly.
`<name>.md5` is **not a digest of anything served** — neither the archive nor the
unpacked file:

| | sidecar claims | `.7z` actually | unpacked actually |
|---|---|---|---|
| `data` | `876189c4…` | `451eae32…` | `f5415ec5…` |
| `model-alpha` | `0fb1b2a8…` | `854af385…` | `e3721dbb…` |

It is an opaque build token the engine only ever compares for equality, so it
works for drift detection but cannot be used to verify a download.

All three models installed = ~101 MB down, ~240 MB resident.

### Drift detection

`Last-Modified` is **useless** — every file reports the same timestamp because the origin deploys the bundle atomically to S3. Real build times are the mtimes *inside* the archives (on 2026-09-20: `model-alpha.dat` 04:46, `data.db` 16:27).

Use the **ETag** (content hash, `"b31bbf56...-2"`, the `-N` suffix is the multipart count) to detect genuine changes.

---

## 2. What's inside `core.wasm`

Linked libraries, from `strings`:

- **OpenCV** (59 hits, `cv::impl`, `opencv_storage`) — detection + rectification
- **SQLite** (`sqlite_master`, `sqlite_schema`, `sqliteX_` prefix)
- **LZMA** — it unpacks its own 7z blobs
- **ncnn** — Tencent's inference runtime, named outright in `[rec model] descript_nn: ncnn extract failed or empty output`. The generic-looking `GEMM_TransposeBlock`, `GEMM_3_T`, `Softmax`, `KERNEL_SYMMETRICAL/ASYMMETRICAL`, `quantization_value`, `NEON_DOTPROD`, `NEON_FP16`, `NEON_BF16`, `simd_helpers` are all ncnn internals
- **Boost** (regex, tokenizer, hex), **nlohmann/json**, **spdlog** — from `/tmp/delverx/third-party/…` paths left in the binary, which also give away the source tree layout (`/tmp/delverx/src/models/`, `/tmp/delverx/vision/src/cvt/detection.hpp`, `/tmp/delverx/vision/src/hash.h`)

NEON symbols in a wasm build indicate one C++ codebase shared with the native Android/iOS builds.

### No OCR. Anywhere.

Zero hits for `tesseract`, `leptonica`, `ocr`, `charset`, `unicharset`, `lstm`, `glyph`, `font` — in the JS bundle *and* inside `core.wasm`. Any text recogniser needs charset tables; there are none.

This is the structural explanation for the reprint/printing-accuracy weakness: recognition is pure learned visual matching on the card image, and the collector number is never read. A bigger model tier cannot fix a missing input.

---

## 3. The model format

`model-alpha.dat` is a bespoke binary. No magic for ONNX, TFLite, GGUF, SafeTensors, PyTorch, or HDF5.

```
offset 0:  3b 94 46 65   magic/version
offset 4:  b9 00 00 00   185
offset 8:  33 33 33 3f   float32 0.7      <- threshold
offset 12: 20 00 00 00   32
offset 16: 08 00 00 00   8
offset 20: 20 00 00 00   32
offset 24: 82 00 00 00   130
offset 28: 82 00 00 00   130
offset 32: 3c 00 00 00   60
offset 36: 3c 00 00 00   60
offset 40: 17 00 00 00   23
offset 44: 17 00 00 00   23
```

Hand-rolled little-endian int32 serializer with a float32 confidence threshold in the header.

**Not encrypted** — gzip -9 still strips 22% (33,980,684 -> 26,640,289, ratio 0.784). Protection is `_rec_set_jwt_token` at the engine level, not crypto on the weights.

### The visual index is inside the model blob

`data.db` contains **no feature vectors** — it is a pure catalogue (schema below). So the embedding index must live in `model-*.dat` alongside the network.

**[inference]** ~60k unique artworks x 256 dims x fp16 ~= 30 MB, which is nearly all of alpha's 34 MB; gamma's 104 MB lands near 768-dim. If right, the three tiers are largely embedding width over the same card set, and the CNN is a small fraction of each file. Not verified.

---

## 4. `data.db` schema (44.4 MB SQLite)

```
cards          124,553   _id, name->names, edition->editions, number TEXT, rarity,
                         multiverseid, foil, promo, digital, price_mid, price_foil,
                         tcg_productid, scryfall_id, cardmarket_id, tcg_sku,
                         frame, finish, alt, artist, flavor_name, scan_type,
                         price_{mid,foil}_{1d,7d,30d,90d,365d}
names           43,008   name, mana, cmana, type, color, power, toughness, loyalty,
                         legal, restricted, reserved, name_full, rules_id
editions         1,065   name, release_date, tl_abb, mtgo_id, online, symbol_img
rules           33,697
rulings         33,629
cards_rulings   82,224
alt_names          947
card_map         6,541
symbols             70
finish_types         5
settings             1
```

Data provenance is visible in the columns: `scryfall_id`, `tcg_productid`, `cardmarket_id`, `multiverseid`. Catalogue from Scryfall, prices from TCGplayer and Cardmarket.

---

## 5. WASM module surface

```
Import[61]:  1 x memory  (initial=16384 max=32768 shared)  <- a.a
             60 x func                                     <- a.b .. a.ga
             0 tables, 0 globals
Export[189]
Tag[2]:      tag[0] sig=0, tag[1] sig=0   (internal: neither imported nor exported)
```

Memory: **1 GiB initial, 2 GiB max, shared** — threads proposal is mandatory.

### Minified names are positional, not hashed

Binaryen consumes its alphabet in strict import order:

```
memory -> a
func 0 -> b     func 24 -> z     func 50 -> Z
func 1 -> c     func 25 -> A     func 51 -> _
...                              func 52 -> $
                                 func 53 -> aa, ba, ca, da, ea, fa, ga
```

`a.q` means "import #15". Nothing is scrambled per build. **Failure mode is a silent off-by-one**: insert one import mid-list and every later name shifts by one slot, so a shim links successfully but misroutes calls.

### Name-independent fingerprint

Bind to this, not to names. Hash it at startup and refuse to run on mismatch.

```
60 funcs: sig3x10 sig6x9 sig0x8 sig4x7 sig2x4 sig16x4 sig15x4
          sig71x2 sig7x2 sig63x2 + singletons 1,21,33,38,62,72,97,98
memory:   16384 / 32768 shared
```

Types in use:

```
type[0]  (i32) -> nil                                type[16] () -> nil
type[1]  (i32, i32) -> nil                           type[21] (i32, i64) -> i32
type[2]  (i32) -> i32                                type[33] (i32, i64, i32) -> i32
type[3]  (i32, i32) -> i32                           type[38] (i32, i64, i32, i32) -> i32
type[4]  (i32, i32, i32) -> i32                      type[62] (i32,i32,i32,i32,i32,i64) -> i32
type[6]  (i32, i32, i32, i32) -> i32                 type[63] (i64, i32) -> i32
type[7]  (i32, i32, i32, i32) -> nil                 type[71] () -> f64
type[15] () -> i32                                   type[72] (i32,i32,i32,i32,i64,i32,i32) -> i32
                                                     type[97] (i32 x7) -> f64
                                                     type[98] (i32, f64) -> i32
```

**[inference]** `() -> f64` x2 = `emscripten_date_now`/`get_now`; the i64 ones = WASI-shaped `fd_seek`/`fd_read`/`fd_write`; `(i32 x7) -> f64` = `_localtime_js`/`_mktime_js` family.

The import *set* moves only when their C++ features or emsdk version change — not on the daily model/data rebuild. **[inference]** so a name mapping likely survives months, then breaks hard once.

---

## 6. Running it headless in Node

`core.js` is served standalone at `/core.js`. It is minified but **keeps original symbol names** (`createCore`, `ENVIRONMENT_IS_NODE`, `Module`, `PThread`) and is UMD (`module.exports = createCore`). Node 24 works.

### Four shims required

**1. `Worker` -> `worker_threads` bridge.** The module preallocates a **32-thread pool** at startup (`pthreadPoolSize = 32`). `_scriptName` is `undefined` in Node, so `Module.mainScriptUrlOrBlob` **must** be set to the `core.js` path or `allocateUnusedWorker` fails immediately.

The pthread worker self-invokes: `var isPthread = globalThis.name == "em-pthread"; isPthread && createCore();`. So the bootstrap just needs `globalThis.name`, `WorkerGlobalScope`, `self`, `postMessage`, `addEventListener` before `require`ing core.js.

**Critical:** the main thread posts the pthread `load` command before `core.js` assigns `self.onmessage`. Buffer incoming messages until a handler exists and flush, or the thread dies with a bare `undefined` and no diagnostic.

**2. `instantiateWasm`.** This build declares `var wasmBinary;` as a bare local — the `Module['wasmBinary']` wiring is stripped for the web-only target, so passing `wasmBinary` does nothing. `Module["instantiateWasm"]` *is* read and is the only injection point:

```js
instantiateWasm(imports, cb) {
  WebAssembly.instantiate(WASM, imports).then((r) => cb(r.instance, r.module));
}
```

**3. `global.fileEvents` EventEmitter.** The shipped `EM_ASM` block (`ASM_CONSTS[742224]`) is:

```js
if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent("file-written"));
else global.fileEvents.emit("file-written");
```

**They ship a Node branch in production.** Somebody at Delver runs this engine headless. Without the emitter the file-written hook throws, and since that is the "job output ready" signal, every job after the first silently times out. This was the single hardest bug in the exercise.

**4. Shutting down.** Nothing on the `Module` object exposes `PThread`, so there is
no supported way to stop the 32-thread pool from the outside. The pool keeps the
node event loop alive, so a process that boots the engine never exits on its own.
The `Worker` shim has to keep its own registry of spawned threads and terminate
them; `engine.close()` does that.

### Non-issue

`ENVIRONMENT_IS_NODE` appears exactly once — in its own declaration. It is never branched on. Vestigial; ignore it.

---

## 7. Job protocol

Most exports return a `requestId`.

- `requestId === 0` -> synchronous, result already in `main.output`
- otherwise -> result appears as `worker_N.output`, N cycling **0..99**

Results are **MessagePack**, not JSON. They must be consumed **in strict N order**, each acked with `_process_next(msg.id)` and then `FS.unlink`ed. Shape:

```json
{"id": 3, "status": "success|error|running", "progress": 0-100,
 "message": "...", "data": {...}}
```

`status: "running"` entries are progress updates on the same channel and must not
consume the pending entry.

**A slot nobody is waiting for still has to be drained.** If `worker_N.output` holds
a result whose `id` matches no in-flight request, it must be unlinked and `N`
advanced anyway — but *without* acking it — or every later result is stuck behind
it and the channel wedges. The app does exactly this; so does `JobQueue` in
`js/engine.js`.

Polling is unnecessary. The `file-written` event from shim #3 fires whenever the
engine writes an output file, so the drain is event-driven with a backoff timer
only as a safety net.

Separately, `_rec_init_rec()` signals completion by creating `recognition.output`
(JSON), and live detection results land in `recognition.json` as
`{det: [...], cards: [...]}` — see section 12.

---

## 8. Bootstrap sequence (verified working, end to end)

```js
_setup_system()                                  -> {status:"success"}
_installation_status()                           -> {installed, hasUpdate, hasUserDb}
// write opfs/cache/data.7z + data.md5 + data.size + version.txt
_install(stringToNewUTF8("opfs/cache/data.7z"))  -> {data:{installed:true}}

// _install leaves the unpacked catalogue at opfs/data.db. Read it back out,
// drop the file, and hand the BYTES to _initialize.
const db = FS.readFile("opfs/data.db"); FS.unlink("opfs/data.db");
_initialize(dbPtr, db.length)                    -> {status:"success"}

_rec_init_rec()                                  -> creates recognition.output {"status":"success"}
_rec_alpha_init(modelPtr, modelLen)              -> {status:"success"}
_rec_set_model(0)                                -> {status:"success"}
```

**Model enum: `ALPHA=0, LAMBDA=1, GAMMA=2`.**

### The three things that make or break it

**1. `_initialize` is the `sqlite3_deserialize` call.** It takes the *unpacked*
`data.db` bytes, not the `.7z`. Handing it the archive returns a vacuous success
while leaving the `data` schema empty — that was the blocker in every earlier
attempt. `_install` takes a **path string**; `_initialize` takes **bytes**. They
are not interchangeable and neither complains.

**2. The deserialise buffer must stay allocated.** SQLite uses it in place as the
live backing store for the `data` schema. Freeing it after `_initialize` returns
turns every later query into `file is not a database`. The app mallocs it once
and holds the pointer for the lifetime of the page; so does `js/engine.js`,
releasing it only in `close()`. The *model* buffer is the opposite — copied
internally, safe to free as soon as init reports back.

**3. `_rec_init_rec()` must come before the model init.** It brings up the
recognition subsystem and clears whatever model was selected. Run it after
`_rec_alpha_init` + `_rec_set_model` and everything still reports success, but
`_rec_best_detection` returns `modelLoaded: false` forever.

### Where the `data` schema lives

`PRAGMA database_list` reports:

```
0  main  /opfs/user.db
2  data  (empty filename)
```

The empty filename is not a bug — the engine runs `ATTACH DATABASE ':memdb:' AS
data` and then deserialises the 44 MB catalogue into it. `main` is `user.db`, the
local collection, which `_initialize` creates on first run (this is what
`hasUserDb` reports). Queries must therefore be schema-qualified: `data.cards`,
not `cards` — unqualified `cards` resolves to the *user's* empty collection table.

Other gotchas:
- Version sidecars must exist in `opfs/cache/` or install fails with `failed to check version`.
- FS layout is `/opfs` and `/opfs/cache`, mirroring the browser's OPFS mount. `_initialize` also creates `/opfs/versions/`, `/opfs/images/`, `/opfs/current_version` and `/opfs/version.md5`.
- `_installation_status()` returns `data: null` once `_initialize` has run.

### Token gating on the tier init calls (observed)

`_rec_alpha_init(ptr, len)` takes **no token** and initialises clean in 0.3s. Only
`_rec_lambda_init(strPtr, ptr, len)` and `_rec_gamma_init(...)` take a JWT string.
`_rec_set_jwt_token(strPtr)` exists separately.

There is also a `_rec_<tier>_install(pathPtr)` per tier, which unpacks
`model-<tier>.7z` to `/model-<tier>.dat` using the engine's own LZMA. Using it
would remove the need to unpack the model archive outside the engine.

---

## 9. The engine is the whole application

189 exports, 169 of them functions. Beyond `_rec_*`:

```
_card_*             30+ fns  create/delete/move/merge/quantity/prices/editions/trade
_list_*             20+ fns  create/rename/merge/format/visibility/background
_sync_*             13 fns   dlens import/export, v2 export, merge, lock, shared db
_export_*            7 fns   formats, fields, generate
_advanced_search_*   4 fns
_friend_*            5 fns   incl. _friend_db_attach
_prices_*            3 fns   incl. _prices_decompress_and_attach
_images_*            5 fns
_sql_query / _sql_exec       direct SQLite access
_setup_system / _install / _initialize / _installation_status
_process_next / _cancel_query / _versions_get / _system_stats_v2
```

The Svelte layer is a thin UI shell over a C++ application core. That reframes the 8.5 MB: it is not a recogniser, it is the product.

---

## 10. How the remaining questions were answered

All three open questions from the first pass are closed.

### The exception is readable now

`core.wasm` defines two tags and exports neither, so `e.is(tag)` / `e.getArg(tag, 0)`
had no handle to work with. The fix is a binary patch: append two entries to the
export section, bump its count LEB and its section-size LEB. That is
`export_internal_tags` in `src/wasm.rs` — **+18 bytes, `wasm-tools validate` clean,
section layout byte-identical otherwise**. `Artifacts::load` applies it in memory at
load; nothing on disk is modified.

With the tags exposed, `getArg` yields the pointer `__cxa_throw` was given. The
`__cxa_exception` header sits in the words just before it, so scanning that
window rather than hardcoding an ABI offset gets both the mangled type name and
`what()`:

```
typeName: St13runtime_error
message:  "Failed to get editions. no such table: data.editions"
```

### "out of memory" was never about memory

`_sql_query` returning `failed to execute sql ... out of memory` is
`sqlite3_errmsg()` on a **NULL database handle** — that is the literal string
SQLite returns in that case. The handle was null because `_initialize` had never
run. Once it does, the same queries return rows. Nothing was ever short of memory.

### `hasUserDb: false` was a symptom, not a cause

The earlier inference that the recogniser needed `/opfs/user.db` to exist was
right about the correlation and wrong about the direction. `_initialize` creates
`user.db` *and* deserialises the catalogue *and* is the step that was missing.
`hasUserDb` was just the most visible thing it hadn't done yet.

### Method note: the app bundle is the oracle

Guessing call order from export names stalled. `mtg.delver.app` is SvelteKit with
34 reachable `/_app/immutable/**.js` chunks, and the engine wrapper is in the
clear inside them — minified, but with the whole bootstrap intact:

```js
if (await install(core, path)) {
  const db = core.FS.readFile('opfs/data.db');
  store('data', db);
  core.FS.unlink('opfs/data.db');
}
await initialize(core);   // _initialize(dataPtr, dataLength)
```

Everything in sections 8 and 12 was read out of that bundle and then confirmed
against the running engine. When a black box ships its own caller, read the
caller.

---

## 11. Files here

```
core.js core.wasm            fetched from mtg.delver.app
data.7z data.db              card catalogue
model-alpha.dat              the alpha model
data.md5 data.size version.txt model-alpha.md5 model-alpha.size

.fixtures/fetch-cards.sh     reference scans from Scryfall

src/lib.rs                   the Rust API: Engine, config, result types
src/sandbox.rs               the isolate: every op the blob can reach, and the log sink
src/worker.rs                the pthread pool as OS threads and isolates
src/pump.rs                  driving the engine from the host
src/wasm.rs                  import fingerprint + the tag-export patch (+18 bytes)
src/job.rs                   the job protocol on the wire: MessagePack in, JSON out
js/bootstrap.js              browser globals over the ops: console, timers, crypto
js/main-prelude.js           Worker -> host-spawned isolates
js/worker-prelude.js         the pthread side of that shim
js/engine.js                 the wrapper: bootstrap, SQL, recognition, job protocol
examples/                    query.rs, recognize.rs
tests/                       end-to-end, plus the accuracy numbers the Node probe set
```

Two earlier layers are gone rather than archived: the first pass's scratch scripts
(`boot*.js`, `patch*.js`, `probe.js`, `scan.js`, `exc.js`, `trace.js`, `load.js`,
`exports.js`, `excheck.js`), and the Node harness that superseded them (`lib/`,
`shim.js`, `worker-boot.js`, `index.js`). Neither is recoverable, which is why this
document is written to stand on its own: everything they established is above.

Run: `cargo run --example query`, `cargo run --example recognize`, `cargo test`.

---

## 12. Recognition protocol

Two entry points, and they do different jobs.

### `_rec_best_detection(ptr, height, width)` — locate only

Returns a job whose data is `{corners: [x0,y0,x1,y1,x2,y2,x3,y3], modelLoaded}`,
in image pixel space. This is the **crop detector**: it finds the card quad so the
UI can draw a rectangle. It does not identify the card. `modelLoaded: false` means
the model was not selected after `_rec_init_rec` — see section 8.

### `_rec_det_rec(ptr, height, width, firstFrame, maskPtr)` — identify

The real recogniser, built around a live camera feed. Synchronous and returns
nothing; results land out of band:

1. push a frame of RGBA
2. poll `_rec_status()` — `-1` RUNNING, `0` FINISHED_NO_DETECTION, `1` FINISHED_WITH_DETECTIONS
3. on `1`, read and unlink `recognition.json`

`maskPtr` is a caller-owned 16384-byte buffer the detector fills with a
segmentation mask each frame.

```json
{"det": [173,90,173,273,307,277,303,87],
 "cards": [{"trackId":0,"hits":2,"saved":1,"savedOnFrame":1,"cardId":1,
            "dataId":38196,"nameId":19185,"editionId":478,"number":"232",
            "name":"Black Lotus","recConf":32,"setConf":100,
            "similar":[38196,42928,42626,32379,55750,118110,113720,5924,20973,12036],
            "imageId":1,"imageFilename":"opfs/images/00000001.jpg",
            "centroid":[49,49],"location":[36,24,63,24,63,76,36,75]}]}
```

`dataId` joins to `data.cards._id`. `location` and `centroid` are percentages,
not pixels. `similar` is the top-10 candidate list — the embedding index's actual
ranking, which is the most interesting field in the blob. `imageFilename` is a
cropped, rectified JPEG the engine writes into its own FS; it accumulates until
collected.

A tight, edge-to-edge card crop detects **nothing** — the detector needs to see
the card boundary against a background. Compositing the same scan onto a plain
backdrop with margin makes it work immediately.

### Measured accuracy

Six Scryfall scans, composited onto a plain background, alpha tier:

| | |
|---|---|
| Card name | **6/6** |
| Exact printing | **4/6** |
| Time per image | ~280–570 ms after boot |
| Full boot (install + deserialise + model + recogniser) | ~3 s |

Boot was first clocked at ~8 s; re-measured on a warm page cache it is ~2.7–3.1 s in
Node and ~3.8–4.2 s under deno_core (§13). The catalogue is unpacked on every boot —
the engine's filesystem is in-memory, so `_install` never finds a previous run.

The two printing misses are *Llanowar Elves* (Dominaria, not M19) and *Swords to
Plowshares* (Foreign Black Border, not Alpha). Both are same-art reprints.

This is section 2's prediction confirmed empirically: with no OCR anywhere in the
binary, the collector number is never read, so printings sharing an illustration
are not separable by construction. `setConf: 100` on a wrong edition shows the
engine is not even uncertain about it — it is confident about a set it inferred
from pixels alone. A larger model tier cannot fix a missing input.

---

## 13. Running it in Rust, sandboxed

Section 6 gets the engine running in Node, where the downloaded blob has everything
Node has. `src/` runs the same artefacts inside a `deno_core` isolate instead, which
starts from nothing and is handed seventeen ops. Same bootstrap, same job protocol,
same results; the difference is what the blob can reach.

### The four Node shims, restated

| Node (§6) | deno_core |
|---|---|
| `Worker` -> `worker_threads` | `Worker` -> an OS thread running its own `JsRuntime` |
| `Module.instantiateWasm` | unchanged - still the only injection point that works |
| `global.fileEvents` EventEmitter | a two-method object built in `js/bootstrap.js` |
| own registry to kill the pool | same registry, plus `v8::IsolateHandle::terminate_execution` |

Plus one the Node build gets for free: **timers**. deno_core ships none, and emscripten
needs `setTimeout` for the mailbox poke and `__setitimer_js`. They are a map in
`js/bootstrap.js` fired by the host pump, which is enough because every one of them is
a zero-or-near-zero delay used to break a call stack rather than to keep time.

### The pool is the hard part, and deno_core already had the piece

`initMemory` allocates `WebAssembly.Memory({initial: 16384, maximum: 32768, shared: true})`
and `loadWasmModuleToWorker` posts `{cmd: 1, handlers, wasmMemory, wasmModule}` to each
of the 32 workers, with `run()` blocked on `addRunDependency("loading-workers")` until
all of them answer. So the pool cannot be stubbed, and the thing that crosses between
isolates is a structured clone carrying a shared `WebAssembly.Memory` and a compiled
module — neither of which is expressible in plain JS.

V8's `ValueSerializer` handles both through delegate hooks, and deno_core exposes them
as `SharedArrayBufferStore` and `CompiledWasmModuleStore`: hand every isolate the same
pair and `Deno.core.serialize`/`deserialize` transfer the memory and the module
correctly with no custom delegate at all. That turned the expected centrepiece of the
port into two fields on `RuntimeOptions`.

`SharedRef<BackingStore>` is `Send` only when `BackingStore: Sync`, which it is not, so
the stores carry the usual `unsafe impl` — sound here because a *shared* buffer's
backing store is exactly the thing designed to be referenced from several threads.

### Nothing calls the engine; the host turns a crank

The bootstrap awaits a 32-worker handshake, jobs land as files announced by an event,
and recognition settles over several frames. `src/pump.rs` therefore loops — deliver
queued worker messages, fire due timers, let V8 drain microtasks, check whether the
promise settled — rather than awaiting anything. `__probePump()` returns the time
until the next timer so idle threads park on a condvar instead of spinning; 32
spinning threads is not a rounding error.

One hazard from §6 disappears rather than being handled: the Node probe had to buffer
the pthread `load` message, because the main thread posts it before `core.js` assigns
`self.onmessage`. Here inbound messages sit in a host-side queue and are pulled only
once a handler exists, so the window does not open.

### Two more host details

- `_emscripten_get_now` is `performance.timeOrigin + performance.now()`. A
  `performance` object without `timeOrigin` makes `_clock_time_get` write
  `BigInt(NaN)` and the engine aborts during `initRuntime`. Both halves come from a
  clock origin shared across isolates, because the engine compares these timestamps
  between threads.
- Leaving `TextDecoder` undefined is deliberate: emscripten falls back to its own
  UTF-8 decoder, which sidesteps decoding views backed by a `SharedArrayBuffer`.
- The §7 job results stay MessagePack, but nothing in `js/` parses them. The glue
  reads the blob out of the wasm filesystem — the one thing only it can do — and
  passes it to `op_probe_decode_job`, which runs it through `facet-msgpack` against
  the declared shape in `src/job.rs` and returns JSON. A blob that is not a job
  result is now a named decode error instead of whatever object the tag bytes
  happened to build.

### What the blob can reach

Seventeen `op_probe_*` ops, and of those only one touches the host filesystem —
`op_probe_artifact`, which serves bytes by name from an allowlist and refuses
anything else. deno_core's own builtins that reach the host (`op_panic`, `op_print`,
`op_pipe`, the resource-table read/write ops) are disabled by extension middleware.
Unit tests assert that `fetch`, `XMLHttpRequest`, `process`, `require`, `indexedDB`
and friends are undefined in the isolate, and that the `op_probe_*` surface is
exactly what was declared.

### Equivalence, measured

Same six fixtures, same tier: 6/6 on card name, 4/6 on exact printing, identical
`dataId`, `recConf`, `setConf` and `similar` rankings. Boot costs about a second more
(~3.8-4.2 s against ~2.7-3.1 s) because 32 isolates each compile `core.js` and there
is no snapshot; recognition is the same wasm doing the same work and lands in the same
~400-500 ms band. `tests/engine.rs` pins both accuracy numbers, so a change that moves
the engine's behaviour fails rather than passes quietly.

### Reached in the Node probe, not carried here

The streaming path (`pushFrame`/`recognitionStatus`/`takeDetections`),
`takeScanImage`, `segmentationMask`, `_system_stats_v2`, and the C++ exception
decoder of §10. The tag patch itself is ported and applied — `src/wasm.rs` — but
nothing reads the tags back out yet, so a C++ exception is still opaque here. §10
records how to read one when that is worth doing again.

---

## 14. Standing caveats

- This is the **alpha tier only**. The JWT gate on lambda/gamma was not touched.
- Minified import names are positional and build-specific. `src/wasm.rs` hashes the import surface by arity, type histogram and memory limits instead, and `Engine::open` refuses to start on a mismatch. Current value: `3411ecc782a61347` — independently reproduced byte for byte by the Node probe's own implementation before that was removed.
- Upstream rebuilds can land at any time and nothing about the blob is contractually stable. Over 2026-09-20/21 no rebuild landed, so "daily" is softer than first assumed — but the fingerprint guard, not the cadence, is what makes that safe to ignore.
- The tag patch is applied in memory at load, by both harnesses. `core.wasm` on disk is never modified.
