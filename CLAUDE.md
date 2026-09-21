# progress-engine

Two products, one workspace. Read NAMES_FOR_FUTURE.md for why they are named
what they are and where a third would go.

**Ichormoon Gauntlet** (`crates/ichormoon-gauntlet/`, binary `gauntlet`) is what
most of this file is about: a Magic: The Gathering draw-probability engine. It
answers *how often does this deck do this by turn N* by exact enumeration, with
a sampling engine alongside as a cross-checking oracle.

**Gitaxian Probe** (`crates/gitaxian-probe/`) is card scanning - a host that
runs Delver X's downloaded recognition engine inside a deno_core sandbox. It
shares the workspace and nothing else yet; its own README and FINDINGS.md are
the authority on it, and the notes below about decks, criteria and the two
engines do not apply to it.

## Read before changing behaviour

- **[VISION.md](VISION.md)** — the product authority. What the tool is, what it
  refuses to be, the two north-star questions, and the principles every design
  decision is judged against. Read it in full before adding a feature, changing
  what a number means, or deciding whether to refuse a question.
- **[HANDS.md](HANDS.md)** — sixteen worked seven-card hands as executable
  specification. Most are tests; each says whether it is. Reach for it when
  touching mana, land drops, zones, routing, or query semantics: it is where
  "this hand should produce this number" is settled.
- **[README.md](README.md)** — the user-facing doc, and where tracked numbers
  live.
- **[NAMES_FOR_FUTURE.md](NAMES_FOR_FUTURE.md)** — the progress-engine family:
  which names are spent, which are banked, and the Scryfall art-tag rule a new
  one has to pass. Read it before naming a crate, a binary or a sibling repo.

## Build and test

Everything goes through the flake devshell, which carries `jq` and
`cargo-nextest` as well as the toolchain:

```
nix develop --command cargo test --all
nix develop --command cargo clippy --all-targets -- -D warnings
nix develop --command cargo fmt --all
```

**Run `cargo fmt --all` before every commit.** CI is `nix flake check`, whose
four checks are fmt, clippy, test and build; a fmt failure aborts the others, so
an unformatted commit reports red without ever having run the tests. This has
hidden broken clippy and tests across four commits before.

The probe adds two wrinkles. Its V8 comes from a fixed-output derivation in the
flake pinned to the `v8` crate version *and* the feature variant deno_core asks
for, so a bump to either needs the hash re-prefetched - `flake.nix` says how.
And its engine tests skip when the upstream blobs or the card fixtures are
missing, which is every sandboxed build: run them with `PROBE_REQUIRE_ENGINE=1`
before claiming its accuracy numbers, or a green suite has checked nothing.

## Verifying a change

- **Observe it, then claim it.** Run the CLI and read the real output. A number
  is the deliverable, so "the tests pass" is not evidence that the number is
  right.
- **Measure on the real decks.** `decks/` holds two Commander lists,
  `lantern.txt` and `loam.txt`, with criteria files beside them. Report group
  counts, composition counts and wall times from a run rather than estimating
  them; `enumerations` in the JSON output carries them.
- **`decks/index.jsonl` carries all eight oracle tags**, so the committed
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
