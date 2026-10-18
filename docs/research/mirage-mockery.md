# Mirage Mockery: printing custom cards

A custom card is a `.card.toml`. Mirage Mockery renders it to a PNG or a PDF,
lays cards out on sheets to print and cut, and checks two things first: that
it is not a real card, which is Phyrexian Metamorph's to print (a refusal,
[ADR-0039](../adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)),
and whether Vivisurgeon's Insight can read its rules text (a warning).

The second check is why this belongs in progress-engine rather than being
another card maker. A homebrew card whose text Insight reads is Magic the
family understands. Chancellor will eventually be able to play it, and
Gauntlet to count it.

## How it renders

[Typst](https://typst.app), as a library: `typst`, `typst-pdf` and
`typst-render`, 0.15.1. The template is `core/src/card.typ`. Rust hands it the job
as JSON in a closed world: the template, the job, the symbols and the art, with
no disk or network access. Card text is therefore never spliced into Typst
source.

Measured in a throwaway prototype before building it (2026-10-17):

- Scryfall's symbol SVGs set inline in rules text, at the text's baseline.
- **Rules text shrinks to fit its box**, a quarter point at a time from 9pt
  down to a floor of 5pt, which is what printed cards do. The prototype's first
  render without this ran off the bottom of the box. The template measures the
  laid-out text against the box with `layout` and `measure`.
- One card, 63 × 88 mm, is a page. A sheet takes Phyrexian Metamorph's
  parameters: paper (A4, A3, Letter, Legal), gap, margin and cut marks, with as
  many cards as fit, centred. A4, Letter and Legal hold 9 and A3 holds 16. The
  policy's notice goes in whichever margin has more room outside the marks.
- The sheet is Typst's, not `metamorph-sheet`'s. Measured on the same four
  cards: 48 KB with vector text, against 610 KB as 300 ppi JPEGs through
  `metamorph-sheet`, whose text is soft and fringed by compression. CPU was
  0.84 s against 9.3 s, though quality decided it.
- `mockery-core` builds for `wasm32-unknown-unknown`: Typst does, its fonts
  are compiled in, and the network and argument parsing are `mockery-cli`'s.
  Getting there needed Insight's ureq made native-only too. Insight only
  downloads from its binary, but rustls' ring in its library's tree had
  quietly broken Chancellor's wasm build as well. The flake's `wasm-libs`
  check now builds both.
- Checking the index for a real card under another name parses only the cards
  with the same mana cost, found in the index's text before any JSON is
  parsed (`IndexFile::costing`). Parsing all 35k took 4.5 s of CPU; the check
  now takes 0.4 s, nearly all of it opening the index.
- Every card the format accepts prints on one page or is refused for space,
  naming the name, the type line or the rules text: a property test over
  generated cards (any mix of types, so a Class creature or a Saga battle too),
  96 cases in 7 s optimised.

## The card file

```toml
name = "Proofsheet Drone"
cost = "{2}{U}{U/P}"
type = "Artifact Creature — Phyrexian Drone"
rules = """
Flying
{1}{U}, {T}: Put an oil counter on target artifact. Activate only as a sorcery.
"""
flavor = "…"
pt = "2/3"          # or loyalty = "4", or defense = "5"
art = "drone.png"   # relative to this file
artist = "…"
```

The corner holds at most one of `pt`, `loyalty` and `defense`, and only the
one the type line asks for: a creature or Vehicle (CR 208.1, 301.7a), a
planeswalker (CR 306.5a) or a battle (CR 310.4a). A creature without `pt` is
an error. The frame colour comes from the cost's colours (CR 105.2): two or
more colours are gold, and none is land, artifact or colorless by the type
line.

## Next

- Art cropping control (focus point), set symbol and rarity, collector line.
- Double-faced and split layouts; saga and class frames.
- Insight's reminder text: a keyword Insight knows can get the reminder text
  printed beside it on request.
- A set file: many cards, one sheet run, numbered.
- ~~Curator: the library builds for wasm, so the browser can render a card the
  same way.~~ Done: `/cards` ([ADR-0040](../adr/0040-curator-prints-proxies-and-custom-cards-in-the-browser.md)).
- Layouts by what a card has, never by a list of frames: chapters for a Saga,
  levels for a Class, a corner for a creature, each placed where it fits. A
  Class creature is levels and a corner, with no frame drawn for that pair.
- A deck with custom cards in it: Metamorph prints its real cards and Mockery
  its custom ones, on one sheet.
