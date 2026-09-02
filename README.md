# progress-engine

Draw-probability tests for Magic: The Gathering decklists.

Named for Jin-Gitaxias's faction of New Phyrexia, on the grounds that obsessively
recalculating whether your deck is perfect yet is blue-aligned behaviour.

## Why

Ask a simple question about a deck — *"how often do I have a white source in my opening
hand?"* — and you can get an answer that is confidently wrong in two different ways.

**Miscategorised cards.** In the session that produced this tool, Kor Haven was counted as a
white source because a regex matched the `{W}` in its *activation cost* rather than its mana
production. Thaumatic Compass was counted as a land when it is an artifact until you control
seven lands. Both errors moved a headline number, and neither announced itself.

**Unstated definitions.** The same deck was asked "what are the odds I have an evasive
connector and a way to arm my commander by turn 5?" and answered **31.0%** and **39.0%** — a
7.5-point spread that came entirely from one analysis counting 8 cards as "connectors" and
another counting 12. Both were defensible. Neither was written down.

The fix is to stop keeping these definitions in your head. Write them as queries, write the
thresholds you care about as assertions, and re-run them when the deck changes:

```js
criterion("t1 dork into t2 commander", (t) =>
  t(1).count('t:land') >= 1 &&
  t(1).count('cat:"Mana Dork"') >= 1 &&
  t(2).count('t:land') >= 2
, { atLeast: 0.55 });
```

That is a unit test for a deck. Change a card, run it again, see which assertions moved.

## How it works

Probabilities are **exact**, not simulated. Cards are grouped by which of your queries they
match, and the tool enumerates count-vectors over those groups, weighting each by its
multivariate hypergeometric probability. There is no shuffler, so there is no sampler bias to
chase — and it is fast enough that the answer is instant.

The JavaScript API deliberately exposes only `count(query)` rather than the cards themselves.
That constraint is what keeps the engine exact: a criterion is a pure function of how many
cards of each query you drew, so it is evaluated once per possible composition instead of once
per simulated hand.

A `--simulate` mode exists for criteria that genuinely need to inspect individual cards. The
exact engine is its test oracle.

## Status

Early. Working today:

- `progress-engine parse <file> --json` — the canonical Archidekt decklist parser, including
  multi-category lines (`[Big Colorless,Test]`).

Planned, in order: Scryfall-syntax query parser, hypergeometric core, JS criteria, `test`
runner, `--simulate`.

## Decklist format

Archidekt's export format. Quantity and name are the only required parts:

```
1x Sol Ring
3x Plains
1x Lightning Bolt (sos) 267 *F* [Interaction]
1x Myr Battlesphere (tdc) [Big Colorless,Test]
1x Rashmi and Ragavan [Commander{top}]
1x Lurrus of the Dream-Den [Companion{noDeck}]
// line-leading double slashes are comments
```

Notes:

- **Multiple categories** are comma-separated inside the brackets.
- **`//` is a comment only at the start of a line.** Card names contain it —
  `Unstable Glyphbridge // Sandswirl Wanderglyph` — and treating it as an inline marker would
  truncate every double-faced card.
- **Malformed lines are errors, not silences.** A line that cannot be parsed names itself and
  its line number. The predecessor to this parser dropped them quietly, so a typo surfaced much
  later as a mysteriously wrong card total.
- `Commander` and `Companion`/`Sideboard`/`Maybeboard`/`{noDeck}` are matched **per category**,
  so `[Ramp,Commander{top}]` is still your commander.

## Development

```bash
nix develop        # devshell with the toolchain
cargo test
nix flake check    # fmt, clippy -D warnings, tests, build
```

## License

MIT

## Crate layout

A workspace, split so each piece can be understood — and depended on — without
dragging in the others.

| Crate | Responsibility | Knows about |
|---|---|---|
| `pe-stats` | Exact hypergeometric draw probabilities | Nothing. No Magic concepts at all. |
| `pe-decklist` | Parsing Archidekt decklists | Decklist text. No card data. |
| `pe-scryfall` | Card data and Scryfall search syntax | Cards. No decklists. |
| `pe-cli` | The `progress-engine` binary | All of the above. |

The seam worth knowing about is between `pe-scryfall` and `pe-decklist`: a query
can filter on `cat:"Exile Outlet"`, which is decklist data, not card data. Rather
than have the card crate depend on the decklist crate, `CardView` takes
categories as a plain `&[String]`. The CLI is what joins the two, which keeps
both halves independently testable.

`pe-stats` deliberately has no idea what a card is. Its tests are pure
known-answer arithmetic, so a failure there is unambiguously a maths bug rather
than a card-data bug.
