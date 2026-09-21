# progress-engine

A Magic: The Gathering draw-probability engine. It answers *how often does this
deck do this by turn N* by exact enumeration, with a sampling engine alongside
as a cross-checking oracle.

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

## Verifying a change

- **Observe it, then claim it.** Run the CLI and read the real output. A number
  is the deliverable, so "the tests pass" is not evidence that the number is
  right.
- **Measure on the real decks.** `decks/` holds two Commander lists,
  `lantern.txt` and `loam.txt`, with criteria files beside them. Report group
  counts, composition counts and wall times from a run rather than estimating
  them; `enumerations` in the JSON output carries them.
- **`decks/index.jsonl` has no oracle tags** (`updated_at` is null), so it
  cannot answer any question involving mana or the effect library — those read
  `otag:tapland` and `otag:surveil`. Sync a tagged index to `/tmp` first.
  Scryfall card lookups go through `POST /cards/collection`, 75 identifiers per
  request.
- **A previously-exact number that moves needs a justification.** The repo's
  criteria files across both decks and both seats make a sweep you can diff
  before and after; do that and say what you found.
- **The two engines must agree.** Teaching the exact engine something means
  teaching `pe-sim` the same thing. Agreement is asserted at three levels and is
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
