# gitaxian probe

Running the [Delver X](https://mtg.delver.app) MTG card-recognition engine from Rust,
with two execution models behind one API. Natively the downloaded blob runs inside a
deno_core sandbox that builds it a browser out of seventeen ops; on the web
(`wasm32`) it runs as a sidecar in the browser it was written for. See *On the web*.

The engine was worked out first through a Node harness, which is where the numbers and
the call sequence in FINDINGS came from. That harness has served its purpose and is
gone; what it established lives on in the writeup and in `tests/engine.rs`.

**[FINDINGS.md](FINDINGS.md) is the writeup** — architecture, the WASM import/export
surface, the model format, the job protocol, the bootstrap sequence, and what it took
to put all of it behind a V8 isolate with no host access.

## Quick start

The probe is its own cargo workspace, outside the repo root's and outside the flake,
so run cargo from `crates/gitaxian-probe/`:

```sh
cd crates/gitaxian-probe
./engine/.fixtures/fetch-cards.sh   # a few reference scans from Scryfall; needs `magick`

cargo run -p gitaxian-probe-engine --example query      # boot and query the catalogue
cargo run -p gitaxian-probe-engine --example recognize  # identify a card in an image
PROBE_REQUIRE_ENGINE=1 cargo test --workspace           # the accuracy numbers, below
./web-check/run.sh                                      # the same numbers, in a browser
```

Natively there is nothing to download first: `Engine::open` pulls the engine, the catalogue
and the alpha model itself and caches them under `/tmp/gitaxian-probe`. The first run
costs ~39 MB down and ~51 MB on disk; later ones cost one nine-byte request. See
*Fetching and caching*.

Building needs the network twice over: `deno_core` links a prebuilt V8 it downloads,
and `gitaxian-probe-assets` downloads Delver's engine files in its build script. The
tests shell out to ImageMagick 7's `magick`; the web check needs the
`wasm32-unknown-unknown` target, a `wasm-bindgen` CLI matching `Cargo.lock`, python3,
and node with playwright. Each script says what it is missing.

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

## On the web

On `wasm32` the same `Engine` exists, but every call is `async`, the config names
where the files are served rather than where they are cached, and nothing is built to
stand in for a browser. `core.js` loads as a classic script, its 32-thread pool is Web
Workers, `core.wasm` is instantiated beside this crate's own module, and the
file-written hook takes its `window` branch. `js/web.js` is the whole of the glue: the
bootstrap sequence and the job protocol, against the real globals. Rust keeps what it
keeps natively - the fingerprint check and tag patch, the ABI constants, the job
decoder and the types.

```rust
use gitaxian_probe_engine::{Engine, EngineConfig, Image};

let mut engine = Engine::open(EngineConfig {
    base: "/gitaxian-probe/".into(),
    ..EngineConfig::default()
})
.await?;
let found = engine.recognize(&Image { data: &rgba, width, height }).await?;
```

The page has to provide two things:

- **Cross-origin isolation.** The pool shares one `WebAssembly.Memory`, so the page
  needs `SharedArrayBuffer`: serve it with `Cross-Origin-Opener-Policy: same-origin`
  and `Cross-Origin-Embedder-Policy: require-corp`. `open` refuses a page without it
  up front; left to core.js, the boot would hang on a Worker `postMessage` instead.
- **The files, from its own origin.** Delver's origin sends no CORS headers
  (checked 2026-09-27: no `Access-Control-Allow-Origin` on any file, and a preflight
  is a 403), so no other origin can `fetch` them. `gitaxian-probe-assets` is the copy:
  its build script downloads the build pinned in `assets/pin.json`, checks every
  file's sha256, unpacks the weights, and lays the directory out in
  `gitaxian_probe_assets::dir()`. A web build copies that next to its output -
  `gitaxian_probe_assets::copy_to(dest)` from its own build step, or
  `cargo run -p gitaxian-probe-assets --example copy -- <dest>` - and sets `base` to
  wherever it is served.

`open` works on the page or inside a dedicated worker, classic or module; in a worker
there is no `window`, so the glue gives the hook the `global.fileEvents` it falls back
to, exactly as the native sandbox does. The pool's workers cannot be stopped from
outside (FINDINGS §6), so `close` frees the engine's buffers and the workers go with
the page.

The pin is also the web host's version lock. Upstream serves only its current build,
so a pinned build comes from the archive (*The archive*, next), and Delver is asked
only when the archive cannot be reached.
`GITAXIAN_PROBE_ASSETS_FROM=<dir>` takes the files from a directory instead of the
network (still checked against the pin), and `GITAXIAN_PROBE_OFFLINE=1` forbids the
download outright.

### The archive

`ghcr.io/cramt/delver-x` is a public OCI artifact that keeps every Delver build as
one tag, `delver-<version>-<12 hex of its SHA256SUMS>`, with each file its own
blob. `.github/workflows/probe-archive.yml` pushes each new build daily with its own
`GITHUB_TOKEN`, and `../archive/README.md` covers the tag rule and why it is
public. A blob's digest is its file's sha256, so the pin, `assets/pin.json`, is
also the list of blobs to fetch. This repo uses it in four places:

- **Building, outside Nix.** `gitaxian-probe-assets`'s build script reads
  `pin.json` and fetches each file by digest from ghcr.io with an anonymous token,
  before asking Delver. Every file is still checked against the pin, and the build
  recomputes the tag from the table, so one edited by hand fails with the tag it
  should be.
- **Building, in Nix.** The flake reads the same `pin.json`, fetches each blob as a
  fixed-output derivation whose hash is the pin, and hands the directory to the
  build script as `GITAXIAN_PROBE_ASSETS_FROM`. Its `gitaxian-probe-web` package is
  the JavaScript API (`pkg/`) beside the served files (`gitaxian-probe/`), and
  `nix flake check` builds it.
- **Pinning a new build.** `.github/workflows/probe-pin.yml` runs daily. It reads
  the `latest` tag's manifest, rewrites `pin.json` from it with `assets/repin.py`,
  and opens one PR per build. It never merges.
- **Reviewing that PR.** `.github/workflows/probe-web-check.yml` runs
  `web-check/run.sh` on every PR that touches the probe, with ImageMagick-made
  frames. It shows whether `KNOWN_FINGERPRINT` still holds and whether 6/6 and 4/6
  moved. A moved fingerprint or number needs someone to read FINDINGS before the PR
  merges. CI does not run the native accuracy test.

The one secret is `GITAXIAN_PROBE_PIN_TOKEN`, for probe-pin: a fine-grained token
with Contents and Pull requests read/write on this repo. The pin PR is pushed with
it, not `GITHUB_TOKEN`, because a PR opened with `GITHUB_TOKEN` runs no workflows,
and then the web check would never see it. Until the secret exists, probe-pin skips.

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

| | Node probe | Rust, native | Rust, web (Chromium) |
|---|---|---|---|
| Card name | **6/6** | **6/6** | **6/6** |
| Exact printing | **4/6** | **4/6** | **4/6** |
| Boot (install + deserialise + model + recogniser) | ~2.7–3.1 s | ~3.8–4.2 s | ~2.9–3.2 s |
| Recognition, per image after boot | ~280–570 ms | ~410–475 ms | ~280–430 ms |

The Node column is the historical measurement the port was checked against, kept
because it is what makes the Rust column mean anything. Boot costs about a second more
under deno_core: 32 isolates each compile `core.js` themselves, and there is no
snapshot. Recognition is the same work in the same wasm, so it lands in the same
range. The web column is `web-check/run.sh` on 1.83.beta, on the page and in a module
worker (the two agree), with the files served from localhost; its boot includes
fetching them. The browser compiles `core.js` once per worker too, but with its own
code cache. The native accuracy test also passes on 1.83.beta, whose import surface
fingerprints the same as the 1.76.beta these notes were first written against.

Both printing misses are same-art reprints — *Llanowar Elves* placed in Dominaria
rather than M19, *Swords to Plowshares* in Foreign Black Border rather than Alpha.
That is the predicted failure: the engine ships no OCR, never reads the collector
number, and cannot separate printings that share an illustration. See FINDINGS §2.
`tests/engine.rs` pins both numbers, so a port that quietly changes the engine's
behaviour fails the suite rather than the review.

The table is 1.83.beta's. The pin is now 1.89.beta (fingerprint `e7615396c5ece313`),
because upstream stopped serving 1.83.beta. On 1.89.beta, card name held at 6/6 on the
web, but exact printing came out at 2/6 or 3/6 on fixture frames made without ImageMagick,
and the native test has not been rerun. See
[probe-in-curator.md](../../../docs/research/probe-in-curator.md).

It only pins them where it can run. Every engine case needs the upstream blobs, and
the accuracy ones need `magick` too; without either they skip and the suite still
passes, which is not the same claim. Set `PROBE_REQUIRE_ENGINE=1` anywhere these
numbers are meant to hold and a skip becomes a failure. `web-check/run.sh` holds the
web host to the same two numbers, on the page and in a worker, and has no skip: a
missing frame or tool is a failure.

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
| `src/lib.rs` | what both hosts share — models, result types, ABI constants |
| `src/native.rs` | the native `Engine`: deno_core, blocking calls |
| `src/web.rs` | the web `Engine`: the browser's own runtime, async calls |
| `js/web.js` | the bootstrap sequence and job protocol, against real browser globals |
| `src/sandbox.rs` | the isolate: every op the blob can reach, and the builtins it cannot |
| `src/worker.rs` | the pthread pool — OS threads, isolates, termination |
| `src/pump.rs` | driving the engine: deliver messages, fire timers, drain microtasks |
| `src/wasm.rs` | import fingerprint and the tag-export patch, in Rust |
| `src/job.rs` | the job protocol's wire format — MessagePack in, JSON out |
| `src/artifacts.rs` | fetching the upstream blobs, and the cache they land in |
| `js/bootstrap.js` | the browser globals, built on those ops |
| `js/main-prelude.js`, `js/worker-prelude.js` | the two halves of the `Worker` shim |
| `js/engine.js` | the bootstrap sequence and job protocol, in-sandbox |
| `../assets/` | `gitaxian-probe-assets`: the pinned engine files a web build serves |
| `../web-check/` | the web host's acceptance check, in headless Chromium |
| `../bindgen/` | `gitaxian-probe-bindgen`: the web host as a JavaScript class, for Meldweb Curator's scan dialog |

## Threading

On the web there is nothing to say: the engine lives on whichever thread opened it,
and every call is a future. Natively, `JsRuntime` pins its isolate to the thread that built it, so `Engine` is `!Send` and
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

No Delver binaries are committed. The native host pulls them from the origin at run
time; the web host serves the copy `gitaxian-probe-assets` downloads at build time,
which means whoever deploys a page is hosting Delver's engine and weights themselves.
