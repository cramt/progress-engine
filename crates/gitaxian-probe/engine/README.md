# gitaxian probe

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
nix develop                 # rust, imagemagick, wasm-tools, node
./.fixtures/fetch-cards.sh  # grab a few reference scans from Scryfall

cargo run -p gitaxian-probe-engine --example query      # boot and query the catalogue
cargo run -p gitaxian-probe-engine --example recognize  # identify a card in an image
cargo test -p gitaxian-probe-engine                     # the accuracy numbers, below
```

There is nothing to download first: `Engine::open` pulls the engine, the catalogue
and the alpha model itself and caches them under `/tmp/gitaxian-probe`. The first run
costs ~39 MB down and ~51 MB on disk; later ones cost one nine-byte request. See
*Fetching and caching*.

Everything the crate and its scripts shell out to comes from the flake, so they run
the same way everywhere; outside the dev shell the scripts say what is missing.

## Rust API

```rust
use gitaxian_probe_engine::{Engine, EngineConfig, Image};

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
// -> rows are Vec<Vec<String>>: the engine sends every column as text,
//    whatever the column was declared as.

// Recognition takes raw RGBA. Decoding is the caller's problem.
let cards = engine.recognize(&Image { data: &rgba, width, height })?;
// -> [Detection { name: "Black Lotus", number: "232", data_id: 38196,
//                 rec_conf: 32, set_conf: 100, similar: [...], .. }]

let card = engine.card_by_id(cards[0].data_id)?;
engine.close();   // optional — `Drop` does this too; call it to pick the moment
```

`Engine::open` returns only once the catalogue is queryable *and* the recogniser has
reported ready, so there is no window where a returned engine is half-built. It
refuses to run if `core.wasm`'s import surface has changed (see *Drift*).

| Method | |
|---|---|
| `Engine::open(EngineConfig)` | boot. `model` is `Model::Alpha`; the gated tiers carry their own `Jwt` and are refused (see *Scope*) |
| `query(sql)` / `exec(sql)` | direct SQLite against the catalogue and collection |
| `card_by_id(data_id)` | resolve a recognition result to catalogue columns |
| `recognize(&Image)` | identify cards in a still RGBA image |
| `locate(&Image)` | find the card quad only — the crop detector, no identification |
| `tier()` / `version()` / `fingerprint()` | which weights, which upstream build, which import surface |
| `worker_count()` | live pthread isolates; 32 once booted |
| `take_logs()` | whatever any isolate wrote to `console`, tagged by isolate |
| `close()` | free buffers and kill the worker pool. `Drop` calls it |

The streaming path the Node probe exercised — `pushFrame`/`recognitionStatus`/
`takeDetections`, `takeScanImage` and `segmentationMask` — is not carried here;
`recognize()` drives the streaming detector internally instead.

### Fetching and caching

`Engine::open` downloads what it needs. There is no fetch step to run first and no
directory to point it at.

| Fetched | | Cached as |
|---|---|---|
| `core.js`, `core.wasm` | the engine | verbatim |
| `data.7z` + `data.md5`, `data.size` | the catalogue, still packed — the engine unpacks it itself | verbatim |
| `model-<tier>.7z` | the weights | unpacked to `model-<tier>.dat` |
| `version.txt` | the build string | not cached — it *is* the cache key |

`version.txt` is nine bytes and is fetched on every open, because upstream rebuilds on
its own schedule. It namespaces the cache, so a rebuild is a miss rather than a stale
hit. If the origin can't be reached, the newest build already in the cache is used
instead, which is what makes a machine that has run once keep working offline.

Where the bytes live is the `ArtifactCache` trait. The default is `DirCache`, one
directory per build under `/tmp/gitaxian-probe`:

```rust
use gitaxian_probe_engine::{DirCache, EngineConfig, Engine, Source};
use std::sync::Arc;

Engine::open(EngineConfig {
    source: Source {
        cache: Arc::new(DirCache::new("/var/cache/gitaxian-probe")),
        offline: false,   // true: never hit the network, cache must already have it
        ..Default::default()
    },
    ..Default::default()
})?;
```

Implement `ArtifactCache` for anything else — an in-memory map, object storage, a
build-tool cache. Two required methods, `get` and `put`, plus `newest_version` if you
want the offline fallback. The cache owes fidelity: what `put` stored is what `get`
returns, or `get` says it has nothing. `DirCache` gets that by writing beside the
target and renaming in, so a run killed mid-download leaves a stray `.partial` rather
than a truncated file that later reads as a hit.

`Source::origin` moves the whole thing somewhere else — a mirror, or a local file
server for tests.

Note that `<name>.md5` is not checked, because it can't be: FINDINGS §1 has it as an
opaque build token that matches neither the archive nor the unpacked file. `<name>.size`
is real, and the unpacked weights are checked against it on every open.

### Debugging inside the sandbox

`console.log` works in `js/engine.js` and anywhere else that runs in there, including
a pthread isolate. It does not reach a terminal on its own: the isolate has no stdout,
so the line goes through `op_probe_log` into a buffer that `take_logs()` drains.

Set `PROBE_LOG=1` to have every line mirrored to stderr as it happens, which is the
only version that helps when the call you are debugging never returns:

Nothing logs by default, so the output is whatever you added. Drop a line into
`js/engine.js`:

```js
console.log("query", sql, { closed, mask: maskPtr });
```

```console
$ PROBE_LOG=1 cargo run --example query
[main] query SELECT count(*) FROM data.cards {"closed":false,"mask":124950504}
```

Each line is prefixed with the isolate it came from — `main`, or `em-pthread7` for one
of the 32 pool threads.

Under `cargo test` the harness swallows stderr, so pass `--nocapture` to watch a
passing test; a failing one prints the captured lines by itself, worker threads
included.

```sh
PROBE_LOG=1 cargo test -- --nocapture
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
| `op_probe_artifact` | the artefact bytes, by name, from a fixed allowlist |
| `op_probe_decode_job` | MessagePack job results parsed into JSON — the wrapper's, not `core.js`'s |

Seventeen ops in total, listed in `src/sandbox.rs`. deno_core's own builtins that reach
the host — `op_panic`, `op_print`, `op_pipe`, the resource-table read/write ops — are
disabled by middleware rather than left unused. `cargo test` asserts both halves: that
`fetch`, `XMLHttpRequest`, `process`, `require`, `indexedDB` and friends are undefined
inside the isolate, and that the only `op_probe_*` ops present are the declared ones.

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

It only pins them where it can run. Every engine case needs the upstream blobs, and
the accuracy ones need `magick` too; without either they skip and the suite still
passes, which is not the same claim. `nix flake check` is one such place - it has no
network, so it builds the crate and runs the pure tests and nothing here. Set
`PROBE_REQUIRE_ENGINE=1` anywhere these numbers are meant to hold and a skip becomes
a failure.

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
| `src/job.rs` | the job protocol's wire format — MessagePack in, JSON out |
| `src/artifacts.rs` | fetching the upstream blobs, and the cache they land in |
| `js/bootstrap.js` | the browser globals, built on those ops |
| `js/main-prelude.js`, `js/worker-prelude.js` | the two halves of the `Worker` shim |
| `js/engine.js` | the bootstrap sequence and job protocol, in-sandbox |

## Threading

`JsRuntime` pins its isolate to the thread that built it, so `Engine` is `!Send` and
every call blocks its thread - `recognize` for a few hundred milliseconds while the
pool works. Embedding it anywhere with a UI therefore means giving the engine a
thread of its own and talking to it over a channel; that is V8's constraint, not a
choice this crate makes. `EngineConfig` *is* `Send`, so the config can be built
wherever and handed across.

## Scope

`alpha` tier only. `Model::Lambda` and `Model::Gamma` carry the `Jwt` that unlocks
them, but `Engine::open` refuses both: the glue calls `_rec_alpha_init` and selects
model 0 whatever the tier, so booting one would feed lambda or gamma weights to
alpha's init and report success. The `_rec_set_jwt_token` gate was not touched and
resolving the per-tier init export is what it would take.

No Delver binaries are committed — the crate pulls them from the origin at run time.
