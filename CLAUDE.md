# progress-engine

Three products, one workspace. Read NAMES_FOR_FUTURE.md for why they are named
what they are and where a fourth would go.

**Ichormoon Gauntlet** (`crates/ichormoon-gauntlet/`, binary `gauntlet`) is what
most of this file is about: a Magic: The Gathering draw-probability engine. It
answers *how often does this deck do this by turn N* by exact enumeration, with
a sampling engine alongside as a cross-checking oracle.

**Gitaxian Probe** (`crates/gitaxian-probe/`) is card scanning - a host for
Delver X's downloaded recognition engine, with two execution models: natively
inside a deno_core sandbox, and on the web as a sidecar the browser runs
itself. It is a **cargo workspace of its own**, outside Gauntlet's, so `cargo`
at the root does not build it; the flake builds its web half
(`gitaxian-probe-web`) but not its native host, which links a prebuilt V8. Its engine README
and FINDINGS.md are the authority on it, and the notes below about decks,
criteria and the two engines do not apply to it.

**Meldweb Curator** (`crates/meldweb-curator/`) is the deck editor: a
`.deck.toml` in a git repo is the deck, and saving is a commit. The same repo's
`collection.toml` is what the user owns, each copy in one place (a binder, a
box, a deck), edited at `/collection` and saved the same way (ADR-0023). It is three
parts: `meldweb-wasm`, which is `chip-decklist` compiled for the browser; `web/`, a
Vite + React + TanStack Router app in the root pnpm workspace; and `worker/`, the
one Cloudflare Worker that serves the site and trades a GitHub login for tokens
without storing anything (deployed with `nix run .#infra -- apply`, an OpenTofu stack in `crates/meldweb-curator/infra/`). In dev, plain
`pnpm dev` logs in through the real GitHub App on localhost:5173, running the
worker's auth routes inside Vite with the client secret from 1Password
(`web/scripts/dev-auth.ts`); `VITE_MOCK_GITHUB=1 pnpm dev` runs the app against an in-page fake GitHub
(`=no-repo` or `=no-install` start at onboarding), so no worker or app is needed. `MELDWEB_PROBE=1 pnpm dev` adds
a Scan button backed by Gitaxian Probe, to a deck and, scanning continuously, to the collection; it is a dev-only spike that no build
carries, and [docs/research/probe-in-curator.md](docs/research/probe-in-curator.md) says why. Copy Archidekt's
editor before improving on it; [docs/research/archidekt-editor.md](docs/research/archidekt-editor.md)
is what that editor does and the order to build it in. The TypeScript never
reimplements what a decklist is: parsing, `commander` and `outside` come from
Rust, and `web/src/deck.gen.ts` is generated from the wire types
(`UPDATE_TS=1 cargo test -p meldweb-wasm`), with a test that fails when it is stale.

Gauntlet stands on **Reality Chip** (`crates/reality-chip/`), the shared core:
`chip-scryfall` (card index, query syntax), `chip-decklist` (Archidekt parsing)
and `chip-stats` (the hypergeometric walk, which knows nothing about Magic and
must stay that way). Gauntlet's own crates are `gauntlet-criteria` (the exact
engine behind an `Evaluator` trait), `gauntlet-toml` (the criteria format and
the only `impl Evaluator`, shared by both engines), `gauntlet-sim` (the
sampler) and `gauntlet-cli`. README's "Crate layout" says which crate may know
what; respect those boundaries.

## Read before changing behaviour

- **[VISION.md](VISION.md)** — the product authority. What the tool is, what it
  refuses to be, the two north-star questions, and the principles every design
  decision is judged against. Read it in full before adding a feature, changing
  what a number means, or deciding whether to refuse a question.
- **[HANDS.md](HANDS.md)** — eighteen worked hands as executable
  specification. Most are tests; each says whether it is. Reach for it when
  touching mana, land drops, zones, routing, mulligans, or query semantics: it
  is where "this hand should produce this number" is settled.
- **[README.md](README.md)** — the user-facing doc, and where tracked numbers
  live.
- **[NAMES_FOR_FUTURE.md](NAMES_FOR_FUTURE.md)** — the progress-engine family:
  which names are spent, which are banked, and the Scryfall art-tag rule a new
  one has to pass. Read it before naming a crate, a binary or a sibling repo.
- **[CONTEXT-MAP.md](CONTEXT-MAP.md)** and **[docs/adr/](docs/adr/)** — the
  glossary per product and the settled decisions, one per file. Use the
  glossary's words and don't re-suggest what an ADR settled. Both summarise
  VISION.md; where they disagree, VISION.md wins and they get fixed.

## Build and test

Plain `cargo` works: the workspace is Gauntlet, Reality Chip and
`meldweb-wasm`, and `rust-toolchain.toml` picks the toolchain. Only a cargo
that reads that file has the wasm32 target, so `pnpm dev` and a wasm32 clippy
need the devshell; everything native builds with any cargo. The flake devshell (`nix develop`) carries the same toolchain plus
`jq`, `cargo-nextest` and the web app's node, pnpm, biome and wasm-bindgen, and
remains the CI path.

```
cargo test --all
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test -p gauntlet-sim --test acceptance <name>   # one test
cargo run --release -p gauntlet-cli -- test decks/lantern.deck.toml decks/lantern.criteria.toml --index decks/index.jsonl
python3 checker/compare.py   # independent Python Monte Carlo vs target/release/gauntlet; exits 1 on disagreement

# Meldweb Curator, inside `nix develop` (node, pnpm, biome and wasm-bindgen come from the flake)
pnpm install
pnpm dev      # rebuilds the wasm on every .rs save and reloads the page
pnpm check    # biome + tsc;  pnpm test: vitest;  pnpm build: the site
```

`checker/` must stay independent of the engine: write it from the README, HANDS.md
and the rules, never from `crates/`, or it checks nothing.

**Run `cargo fmt --all` before every commit**, and `pnpm fmt` when the web app
changed. CI is `nix flake check`, whose six checks are fmt, clippy, test, build,
the Python checker and `meldweb-web` (biome, tsc, vitest and the site build,
against the crane-built wasm); a fmt failure aborts the others, so an
unformatted commit reports red without ever having run the tests. This has
hidden broken clippy and tests across four commits before.

`meldweb-web`'s node_modules come from `cramt/pnpm2nix`, which fetches each
package by the integrity `pnpm-lock.yaml` records, so a lockfile change needs
no hash in flake.nix.

The probe builds from its own manifest - `cargo test --manifest-path
crates/gitaxian-probe/Cargo.toml --workspace` natively, and
`crates/gitaxian-probe/web-check/run.sh` for the web host in headless Chromium.
Both need the network: V8 is a prebuilt download, and `gitaxian-probe-assets`
fetches Delver's engine at build time against a hash pin, `assets/pin.json`,
from the public archive on ghcr.io and then from Delver, which serves only its
current build (the engine README, *The archive*).
Its native engine
tests skip when the upstream blobs or the card fixtures are missing: run them
with `PROBE_REQUIRE_ENGINE=1` before claiming its accuracy numbers, or a green
suite has checked nothing. The web check has no skip; it fails instead.

## Verifying a change

- **Observe it, then claim it.** Run the CLI and read the real output. A number
  is the deliverable, so "the tests pass" is not evidence that the number is
  right.
- **Measure on the real decks.** `decks/` holds two Commander lists,
  `lantern.deck.toml` and `loam.deck.toml`, with criteria files beside them. Report group
  counts, composition counts and wall times from a run rather than estimating
  them; `enumerations` in the JSON output carries them.
- **`decks/index.jsonl` carries all ten oracle tags**, so the committed
  criteria files answer against the committed index and a clone can reproduce
  every number in them: `gauntlet test decks/lantern.deck.toml
  decks/lantern.criteria.toml --index decks/index.jsonl`. It was built with
  `--from` and carried none until the `sync` in #59; if you rebuild it, check
  `provenance.index_updated_at` is a date rather than null before committing,
  because a tagless index makes `otag:tapland` and `otag:surveil` refuse and
  takes every mana and effect question with them. Scryfall card lookups go
  through `POST /cards/collection`, 75 identifiers per request.
- **A previously-exact number that moves needs a justification.** The repo's
  criteria files across both decks and both seats make a sweep you can diff
  before and after; do that and say what you found.
- **The two engines must agree.** Teaching the exact engine something means
  teaching `gauntlet-sim` the same thing. Agreement is asserted at three levels and is
  the acceptance test for touching the enumeration.

## Conventions

- Commit subjects are lowercase and make a claim about behaviour, not about
  code: `mana: a turn's lands are a budget, and the run names the line that
  spent them`.
- A policy the user declares — mulligan bottoming, selection routing, the land
  drop, which spells a line casts — is **one mechanism**: a declared priority
  over queries. Adding a fifth policy language is the failure VISION.md is
  written against.
- Any run whose numbers depended on a declared policy prints the policy it used.
