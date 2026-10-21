# Mirage Mockery: printing custom cards

A custom card is a `.card.toml`. Mirage Mockery renders it to a PNG or a PDF,
lays cards out on sheets to print and cut, and first asks whether
Vivisurgeon's Insight can read its rules text (a warning). It used to refuse a
real card too; that was dropped
([ADR-0039](../adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).

That check is why this belongs in progress-engine rather than being
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
- **Rules text shrinks to fit its box**, to the quarter point from 9pt
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
- Every card the format accepts prints on one page or is refused for space,
  naming the name, the type line or the rules text: a property test over
  generated cards (any mix of types, so a Class creature or a Saga battle too,
  and every layout below, colour indicators and cards of two parts included).
  96 cases took 7 s optimised with plain text only; the striated layouts lay
  each section out twice, and stepping down a quarter point at a time took
  that to 25 s, so the size is now found by halving: 10 s. A quarter of the
  cases being two faces left it at 9.8 s (2026-10-21).

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
color_indicator = "WR"  # optional: colours a card with no cost
art = "drone.png"   # relative to this file
artist = "…"
```

The corner holds at most one of `pt`, `loyalty` and `defense`, and only the
one the card asks for: a creature or Vehicle (CR 208.1, 301.7a), a station
card, whose power and toughness belong to its last striation (CR 721.2b), a
planeswalker (CR 306.5a) or a battle (CR 310.4a). A creature without `pt` is
an error.

The frame colour comes from the cost's colours (CR 105.2) and the colour
indicator's (CR 204.2): two or more colours are gold, and none is artifact or
colorless by the type line. A land's frame is always a land's, tinted by its
colours or, with none, by the coloured mana its text adds (`{T}: Add {G}.`,
`({T}: Add {B} or {G}.)`): one colour tints it, two blend across it as a dual
land's text box does, three or more make it gold. "Any color" names no colour
and leaves it plain. The colour indicator is a dot left of the type line
(CR 204.1), split corner to corner when it has more than one colour, which a
transform card's back face will need.

Text is set with curly quotes, `don't` as `don’t` and `"…"` as `“…”`; the
file keeps what was typed.

### Cards of two parts

A card with two faces, or two halves, says which: the top level is the front
face (a split card's left half), and the other is a table holding the same
fields.

```toml
name = "The Cleansing of Shinovar"
cost = "{2}{R}{W}"
type = "Enchantment — Saga"
rules = """…IV — Exile this Saga, then return it to the battlefield transformed under your control."""
layout = "transform"     # or "modal", with [back]; or "split", with [right]

[back]
name = "Kaladin, Herald of Second Chances"
type = "Legendary Enchantment Creature — Human Knight"
color_indicator = "WR"
rules = "Flying, vigilance\n…"
pt = "4/4"
```

`card.rs` holds a card as one of four shapes, a single face, a transforming
pair, a modal pair or a split pair, so a `[back]` with no layout, a layout
with no second face, or a split card's half under `[back]` (it has a card
back) is refused by name. Each face is checked as a card is. A split card's
halves each need a cost: every printed one has one, and the card's cost is
the two together (CR 709.4b). A back face may have a cost: Plant a Sapling //
Fully-Grown Treefolk's back costs {G}, and a modal card's faces usually both
do.

- **Transform** (CR 712.2): each face is a card image, marked on the name
  bar's left as printed ones are (CR 712.2a–b), with Mockery's own mark: a
  dark disc with a chevron up on the front and down on the back. A front
  whose back is a creature prints the back's power and toughness in grey
  beside its own corner, or on its type line's end when it has none, as a
  transforming Saga does (CR 712.2c).
- **Modal** (CR 712.3): marked by a tab, one chevron on the front and two on
  the back, dark on the front and light on the back (CR 712.3a–b). Each face
  has a hint bar at its foot's left about the other (CR 712.3c): its
  subtypes, or its card types with none, as Kazandu Mammoth's says
  "Elephant" and Emeria's Call's "Land"; then its cost or, for a land, its
  `{T}: Add …` ability. The bar is dark on the front and light on the back,
  as printed ones are.
- **Split** (CR 709): the two halves side by side on a card turned on its
  side, each drawn as a small card face, read with the card's left edge at
  the foot. Fuse is read from the text, never declared: when both halves have
  a line opening `Fuse`, it leaves both and prints once on a bar across the
  foot of the card, blended from the halves' colours (CR 702.102a). One half
  with fuse keeps it as a line of its own.

Insight reads every face's rules text, each as its own face (CR 712.8,
709.3b). Matched against Scryfall's images of Fable of the Mirror-Breaker,
Delver of Secrets, Emeria's Call, Turntimber Symbiosis, Valki, God of Lies,
Kazandu Mammoth, Agadeem's Awakening, Far // Away, Turn // Burn and Wear //
Tear, with their Oracle text rendered through Mockery beside them. The face
marks, the hint bar and the fuse bar are Mockery's own drawing, not Wizards'
symbols
([ADR-0039](../adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).

Curator's form edits either face: a picker above the form chooses the front
or the back (left or right half), and writing one never touches the other or
the layout. The preview has the same picker for a double-faced card.

### Layouts, from what the text says

`rules` stays Oracle text: paste a card's Oracle text in and it takes its
layout from the markers in it, never from a list of frames
(`core/src/rules.rs`). The template gets the text already read into one of:

- **Plain**: one paragraph after another, art above, text below.
- **Saga**, when a line opens with chapter numerals, `I — …` or `II, III —
  …` (CR 714.2): the art stands down the right and the type line goes to the
  foot (CR 714.1). The text column holds what comes before the chapters (the
  reminder, read ahead) across its top, then a section a chapter ability with
  its numerals as hexagons in a strip down the left, the sections sharing the
  height left over. Lines after the last chapter are a Saga creature's other
  abilities, set with its power and toughness in a box below the type line
  (CR 714.1a). The chapters must count up from I with none missed and none
  out of place.
- **Class**, when a line is a level bar, `{2}{U}: Level 2` (CR 716.2): the
  art stands down the left, the type line at the foot (CR 716.1). The level 1
  section comes first, its reminder set smaller and ruled off; then each level
  bar, its cost on the left and "Level N" on the right, over its abilities.
  Levels count up from 2.
- **Station**, when a line opens `3+ | …` (CR 721.2): the standard layout,
  with each threshold a striation of the text box a step darker than the one
  above, its number on a disc at the box's left edge, and the lines after it
  belonging to it. What comes before the first threshold holds at all times,
  the station keyword among it. The last striation runs to the foot of the box
  and holds the power and toughness. Thresholds must climb.

A text with the markers of more than one of these is refused. These follow
where printed cards put chapters, levels and thresholds; the shapes, colours
and fonts are Mockery's own frame
([ADR-0039](../adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).
They were matched by rendering real cards' Oracle text next to their Scryfall
images: The Eldest Reborn, History of Benalia and Summon: Bahamut; Wizard and
Ranger Class; Exploration Broodship, Uthros Research Craft and The Eternity
Elevator; Khalni Garden, Overgrown Tomb and Dryad Arbor.

## Next

- Art cropping control (focus point), set symbol and rarity, collector line.
- ~~Double-faced and split layouts.~~ Done: transform, modal and split with
  fuse, see *Cards of two parts*. Not yet: adventure, flip, meld, Rooms
  (split with a shared type line, CR 709.5), aftermath, and a planeswalker's
  loyalty abilities laid out as printed.
  ~~Saga and class frames.~~ Done, with station cards, from the text: see
  *Layouts, from what the text says*.
- Insight's reminder text: a keyword Insight knows can get the reminder text
  printed beside it on request.
- A set file: many cards, one sheet run, numbered.
- ~~Curator: the library builds for wasm, so the browser can render a card the
  same way.~~ Done: `/cards` ([ADR-0040](../adr/0040-curator-prints-proxies-and-custom-cards-in-the-browser.md)).
- ~~Layouts by what a card has, never by a list of frames: chapters for a Saga,
  levels for a Class, a corner for a creature, each placed where it fits. A
  Class creature is levels and a corner, with no frame drawn for that pair.~~
  Done: a Class or Saga creature with nothing below its type line carries its
  corner on the type line's end.
- A deck with custom cards in it: Metamorph prints its real cards and Mockery
  its custom ones, on one sheet.
