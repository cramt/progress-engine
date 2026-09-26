# Criteria are TOML data, not a script or a DSL

Criteria used to be JavaScript run in V8. They are now TOML: `[[criterion]]` with `require` clauses of turn, query, zone and count ([#44](https://github.com/cramt/progress-engine/issues/44), `8cf92cf`). There are three reasons. Data is **analysable**: the queries a file asks about are known without running it, which deleted the half-run too-wide refusal of [#36](https://github.com/cramt/progress-engine/issues/36). Data is **pure**, so hashing it as provenance means something. And it is **faster**, because no composition crosses into a JS runtime. The text format is an API for a graphical builder, so it has to round-trip without anyone reimplementing a grammar.

Each clause holds exactly one query, and clauses do no arithmetic, because `count(a) + count(b)` double-counts a card in both groups. `deny_unknown_fields` is load-bearing: without it a misspelled key is silently dropped.

## Considered Options

- **Keep JavaScript.** Rejected for the reasons above.
- **A Scryfall-style one-line grammar** (`turn=5 zone=graveyard count>=1`). Rejected, because a GUI would have to reimplement the grammar.

See [VISION.md: The criteria are data](../../VISION.md#the-criteria-are-data).
