# progress-engine

Two products, one workspace. Read NAMES_FOR_FUTURE.md for why they are named
what they are and where a third would go.

**Ichormoon Gauntlet** (`crates/ichormoon-gauntlet/`, binary `gauntlet`) is what
most of this file is about: a Magic: The Gathering draw-probability engine. It
answers *how often does this deck do this by turn N* by exact enumeration, with
a sampling engine alongside as a cross-checking oracle.

**Gitaxian Probe** (`crates/gitaxian-probe/`) is card scanning - a host that
runs Delver X's downloaded recognition engine inside a deno_core sandbox. It is
**parked**: a failed MVP whose source stays in the tree but is excluded from the
cargo workspace and the flake, so nothing builds or tests it. The root
`Cargo.toml` says how to bring it back. Its own README and FINDINGS.md are the
authority on it, and the notes below about decks, criteria and the two engines
do not apply to it.

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

Plain `cargo` works: the workspace is Gauntlet and Reality Chip only, and
`rust-toolchain.toml` picks the toolchain. The flake devshell
(`nix develop`) carries the same toolchain plus `jq` and `cargo-nextest`, and
remains the CI path.

```
cargo test --all
cargo clippy --all-targets -- -D warnings
cargo fmt --all
cargo test -p gauntlet-sim --test acceptance <name>   # one test
cargo run --release -p gauntlet-cli -- test decks/lantern.txt decks/lantern.criteria.toml --index decks/index.jsonl
python3 checker/compare.py   # independent Python Monte Carlo vs target/release/gauntlet; exits 1 on disagreement
```

`checker/` must stay independent of the engine: write it from the README, HANDS.md
and the rules, never from `crates/`, or it checks nothing.

**Run `cargo fmt --all` before every commit.** CI is `nix flake check`, whose
five checks are fmt, clippy, test, build and the Python checker; a fmt failure aborts the others, so
an unformatted commit reports red without ever having run the tests. This has
hidden broken clippy and tests across four commits before.

The parked probe needed the flake to build at all - a prebuilt V8 pinned by
hash and an Android-patched deno_core symlinked into `vendor/` - which is why it
was taken out of the workspace. If it comes back, so does that wiring, and its
engine tests skip when the upstream blobs or the card fixtures are missing: run
them with `PROBE_REQUIRE_ENGINE=1` before claiming its accuracy numbers, or a
green suite has checked nothing.

## Verifying a change

- **Observe it, then claim it.** Run the CLI and read the real output. A number
  is the deliverable, so "the tests pass" is not evidence that the number is
  right.
- **Measure on the real decks.** `decks/` holds two Commander lists,
  `lantern.txt` and `loam.txt`, with criteria files beside them. Report group
  counts, composition counts and wall times from a run rather than estimating
  them; `enumerations` in the JSON output carries them.
- **`decks/index.jsonl` carries all ten oracle tags**, so the committed
  criteria files answer against the committed index and a clone can reproduce
  every number in them: `gauntlet test decks/lantern.txt
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
