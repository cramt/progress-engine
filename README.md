# delver-probe

Running the [Delver X](https://mtg.delver.app) MTG card-recognition engine headlessly:
a Rust library that runs the downloaded blob inside a deno_core sandbox.

The engine was worked out first through a Node harness, which is where the numbers and
the call sequence in FINDINGS came from. That harness has served its purpose and is
gone; what it established lives on in the writeup and in `tests/engine.rs`.

**[FINDINGS.md](FINDINGS.md) is the writeup** — architecture, the WASM import/export
surface, the model format, the job protocol, the bootstrap sequence, and what it took
to put all of it behind a V8 isolate with no host access.

## Quick start

```sh
nix develop                 # rust, p7zip, imagemagick, wasm-tools, node
./fetch.sh                  # pull engine + card DB + alpha model from the origin
./.fixtures/fetch-cards.sh  # grab a few reference scans from Scryfall

cargo run --example query   # boot and query the catalogue
cargo run --example recognize   # identify a card in an image
cargo test                  # includes the accuracy numbers the Node probe set
```

Everything the crate and its scripts shell out to comes from the flake, so they run
the same way everywhere; outside the dev shell the scripts say what is missing.

## Rust API

```rust
use delver_engine::{Engine, EngineConfig, Image};

let mut engine = Engine::open(EngineConfig::default())?;

// The catalogue is SQLite. `data.` is Delver's 124k-card catalogue,
// `main` is the local collection in user.db.
let rows = engine.query(
    "SELECT n.name, e.name, c.number
     FROM data.cards c
     JOIN data.names n ON n._id = c.name
     JOIN data.editions e ON e._id = c.edition
     WHERE n.name = 'Black Lotus'",
)?;

// Recognition takes raw RGBA. Decoding is the caller's problem.
let cards = engine.recognize(&Image { data: &rgba, width, height })?;
// -> [Detection { name: "Black Lotus", number: "232", data_id: 38196,
//                 rec_conf: 32, set_conf: 100, similar: [...], .. }]

let card = engine.card_by_id(cards[0].data_id)?;
engine.close();   // required — tears down the 32-thread pool
```

`Engine::open` returns only once the catalogue is queryable *and* the recogniser has
reported ready, so there is no window where a returned engine is half-built. It
refuses to run if `core.wasm`'s import surface has changed (see *Drift*).

| Method | |
|---|---|
| `Engine::open(EngineConfig)` | boot; `model` is `Alpha`, `Lambda`/`Gamma` need a `token` |
| `query(sql)` / `exec(sql)` | direct SQLite against the catalogue and collection |
| `card_by_id(data_id)` | resolve a recognition result to catalogue columns |
| `recognize(&Image)` | identify cards in a still RGBA image |
| `locate(&Image)` | find the card quad only — the crop detector, no identification |
| `worker_count()` | live pthread isolates; 32 once booted |
| `take_logs()` | whatever any isolate wrote to `console`, tagged by isolate |
| `close()` | free buffers and kill the worker pool |

The streaming path the Node probe exercised — `pushFrame`/`recognitionStatus`/
`takeDetections`, `takeScanImage` and `segmentationMask` — is not carried here;
`recognize()` drives the streaming detector internally instead.

### Debugging inside the sandbox

`console.log` works in `js/engine.js` and anywhere else that runs in there, including
a pthread isolate. It does not reach a terminal on its own: the isolate has no stdout,
so the line goes through `op_delver_log` into a buffer that `take_logs()` drains.

Set `DELVER_LOG=1` to have every line mirrored to stderr as it happens, which is the
only version that helps when the call you are debugging never returns:

Nothing logs by default, so the output is whatever you added. Drop a line into
`js/engine.js`:

```js
console.log("query", sql, { closed, mask: maskPtr });
```

```console
$ DELVER_LOG=1 cargo run --example query
[main] query SELECT count(*) FROM data.cards {"closed":false,"mask":124950504}
```

Each line is prefixed with the isolate it came from — `main`, or `em-pthread7` for one
of the 32 pool threads.

Under `cargo test` the harness swallows stderr, so pass `--nocapture` to watch a
passing test; a failing one prints the captured lines by itself, worker threads
included.

```sh
DELVER_LOG=1 cargo test -- --nocapture
```

Arguments are rendered one line per call, not as a browser console would: typed arrays
and `ArrayBuffer`s become `[Uint8Array 419430400]` rather than a serialised copy of the
heap, cycles become `[Circular]`, and anything past 2000 characters is truncated.

## What the sandbox actually allows

`core.js` and `core.wasm` are downloaded from a vendor that rebuilds them on its own
schedule. Under deno_core they start
from a bare V8 isolate — no filesystem, no network, no process, no module loader — and
get back only what the Emscripten glue cannot run without:

| | |
|---|---|
| `console`, `performance`, `crypto.getRandomValues`, `navigator` | ordinary web globals |
| `setTimeout` / `clearTimeout` | deno_core ships no timers; emscripten needs them to break call stacks |
| `Worker` | the 32-thread pool, as real OS threads running real isolates |
| `global.fileEvents` | the shipped EM_ASM hook that announces a finished job |
| `op_delver_artifact` | the artefact bytes, by name, from a fixed allowlist |

Fourteen ops in total, listed in `src/sandbox.rs`. deno_core's own builtins that reach
the host — `op_panic`, `op_print`, `op_pipe`, the resource-table read/write ops — are
disabled by middleware rather than left unused. `cargo test` asserts both halves: that
`fetch`, `XMLHttpRequest`, `process`, `require`, `indexedDB` and friends are undefined
inside the isolate, and that the only `op_delver_*` ops present are the declared ones.

Deliberately *not* defined, because `core.js` branches on them: `window`
(`ENVIRONMENT_IS_WEB`, and the file-written hook), `process` (`ENVIRONMENT_IS_NODE`),
`TextDecoder` (emscripten's own UTF-8 fallback avoids decoding `SharedArrayBuffer`
views).

## Accuracy and cost, measured

Six Scryfall scans composited onto a plain background (`.fixtures/fetch-cards.sh`),
alpha tier. Both harnesses produce the same `dataId`, the same confidences and the
same `similar` ranking:

| | Node probe | Rust |
|---|---|---|
| Card name | **6/6** | **6/6** |
| Exact printing | **4/6** | **4/6** |
| Boot (install + deserialise + model + recogniser) | ~2.7–3.1 s | ~3.8–4.2 s |
| Recognition, per image after boot | ~280–570 ms | ~410–475 ms |

The Node column is the historical measurement the port was checked against, kept
because it is what makes the Rust column mean anything. Boot costs about a second more
under deno_core: 32 isolates each compile `core.js` themselves, and there is no
snapshot. Recognition is the same work in the same wasm, so it lands in the same
range.

Both printing misses are same-art reprints — *Llanowar Elves* placed in Dominaria
rather than M19, *Swords to Plowshares* in Foreign Black Border rather than Alpha.
That is the predicted failure: the engine ships no OCR, never reads the collector
number, and cannot separate printings that share an illustration. See FINDINGS §2.
`tests/engine.rs` pins both numbers, so a port that quietly changes the engine's
behaviour fails the suite rather than the review.

## Drift

The engine is rebuilt upstream on its own schedule and the minified import names are
**positional**, so an inserted import silently shifts every later name and a shim
misroutes calls without erroring. `src/wasm.rs` therefore hashes the import surface
(arity, type histogram, memory limits — not names) and `Engine::open` refuses to start
on a mismatch. When that fires, re-verify the ABI against FINDINGS before passing
`allow_unknown_build`.

## What's here

| | |
|---|---|
| `src/lib.rs` | the Rust API — `Engine`, config, result types |
| `src/sandbox.rs` | the isolate: every op the blob can reach, and the builtins it cannot |
| `src/worker.rs` | the pthread pool — OS threads, isolates, termination |
| `src/pump.rs` | driving the engine: deliver messages, fire timers, drain microtasks |
| `src/wasm.rs` | import fingerprint and the tag-export patch, in Rust |
| `js/bootstrap.js` | the browser globals, built on those ops |
| `js/main-prelude.js`, `js/worker-prelude.js` | the two halves of the `Worker` shim |
| `js/engine.js`, `js/msgpack.js` | the bootstrap sequence and job protocol, in-sandbox |
| `fetch.sh` | downloads and unpacks the upstream blobs |

## Scope

`alpha` tier only. `lambda` and `gamma` are gated behind `_rec_set_jwt_token`;
that gate was not touched and isn't in scope here.

No Delver binaries are committed — `fetch.sh` pulls them from the origin.
