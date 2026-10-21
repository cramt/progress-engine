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
  (since 2026-10-21 9.5pt, printed cards' largest, see *The card's height*)
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
  generated cards (any mix of types, so a Class creature or a land planeswalker,
  battles alone or as a Siege's front,
  and every layout below, colour indicators and cards of two parts included).
  96 cases took 7 s optimised with plain text only; the striated layouts lay
  each section out twice, and stepping down a quarter point at a time took
  that to 25 s, so the size is now found by halving: 10 s. A quarter of the
  cases being two faces left it at 9.8 s, and adding aftermath cards and
  Rooms 11–14 s over four runs (2026-10-21).

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
art = "drone.png"   # relative to this file; without it, DiceBear placeholder art
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
layout = "transform"     # or "modal", with [back]; "split", with [right];
                         # "adventure" or "prepare", with [spell];
                         # "flip", with [flipped]; or "meld", with
                         # [counterpart] and [melded]

[back]
name = "Kaladin, Herald of Second Chances"
type = "Legendary Enchantment Creature — Human Knight"
color_indicator = "WR"
rules = "Flying, vigilance\n…"
pt = "4/4"
```

`card.rs` holds a card as one of eight shapes, a single face, a transforming
pair, a modal pair, a split pair, a card with an Adventure or a prepare
spell, a flip card, or a meld pair, so a `[back]` with no layout, a layout
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
  beside its own corner (CR 712.2c). A transforming Saga, with no corner of
  its own, prints it in its last chapter's bottom corner under the chapter
  symbols, as Fable of the Mirror-Breaker does.
- **Modal** (CR 712.3): marked by a tab, one chevron on the front and two on
  the back, dark on the front and light on the back (CR 712.3a–b). Each face
  has a hint bar at its foot's left about the other (CR 712.3c): its
  subtypes, or its card types with none, as Kazandu Mammoth's says
  "Elephant" and Emeria's Call's "Land"; then its cost or, for a land, its
  `{T}: Add …` ability. The bar is dark on the front and light on the back,
  as printed ones are.
- **Split** (CR 709): the two halves side by side on a card turned on its
  side, each drawn as a small card face, read with the card turned a quarter
  clockwise: its left edge at the top and its foot, with the footer, on the
  left, as Scryfall's images of Wear // Tear, Far // Away and every Room
  read. What the halves are is read from them, never declared, so pasting a
  printed card's Oracle text in prints its frame, and a file cannot say one
  frame while its text says another (`SplitKind` in `card.rs`):
  - **Fuse**: when both halves have a line opening `Fuse`, it leaves both
    and prints once on a bar across the foot of the card (CR 702.102a), and
    the halves' name and type bars point at each other, as printed fuse
    cards' do. The bar has the name and type bars' shape (`plate` in
    `card.typ`): D-shaped ends, a rim and a shadow, its fill and its rim
    each blended from the halves' colours, its fill 3.5mm deep as Wear //
    Tear's and Far // Away's are (41 of 745 px), standing over the halves'
    frames' foot. One half with fuse keeps it as a line of its own.
  - **Aftermath** (CR 702.127), when the right half has a line opening
    `Aftermath`: the left half stands upright on the card's top half, a card
    face in small, and the right half, the one cast from a graveyard, lies on
    its side below it, read with the card turned a quarter anticlockwise, its
    name bar toward the card's right edge, as Cut // Ribbons, Never //
    Return, Spring // Mind and Commit // Memory print theirs. The lower
    half's frame is full depth along its middle and steps in round its bars'
    rounded ends, as printed ones' do. It is no `layout` of its own, though
    the card prints in a frame of its own, for the same reason fuse is none:
    Scryfall and Oracle call all 27 aftermath cards (`keyword:aftermath`)
    `split`, and the keyword says which half is which. Refused, by name:
    aftermath on the left half (the half printed upright is the one cast
    from a hand); aftermath with fuse on either half (fuse casts from a hand,
    an aftermath half only from a graveyard, CR 702.127a, 702.102a); and a
    half that is no instant or sorcery, or has a Saga's, Class's or other
    frame's text: all 27 printed ones' halves are plain instants and
    sorceries.
  - **Room** (CR 709.5), when a half's subtypes say Room: the doors side by
    side, each a frame of its own with its name bar and text box, under one
    picture across both, one shared type line (CR 709.5a) and a band with
    the doors' unlock reminder, as Ticket Booth // Tunnel of Hate,
    Bottomless Pool // Locker Room, Dazzling Theater // Prop Room and Smoky
    Lounge // Misty Salon print theirs. How it differs from an instant or
    sorcery split card: the art and the type line run across both doors, so
    the doors part only at the name bars and the text boxes; the type line
    is printed once, dark and see-through over the art with white type,
    rimmed in both doors' colours blended; Oracle gives each door the same
    reminder, "(You may cast either half. …)", and the card prints it once,
    white italic on a dark band across the art under the type line; and
    each door's frame rounds off beside its text box toward the card's
    foot, the box running on to the border, rimmed in the door's colour.
    The reminder is lifted into the band when both doors' text ends with
    the same line in parentheses, and otherwise the band is empty and each
    door keeps its text. The art is the left half's `art`, one picture; an
    `art` on the right half is refused. Refused, by name: a Room half beside
    a half that is not one, halves whose type lines differ (they share one,
    CR 709.5a), a door with a corner or a framed text, and fuse or aftermath
    on a Room. All 28 Rooms on Scryfall (`t:room`) are `Enchantment — Room`
    on both doors.
  - The halves' parts stand where printed ones' do, in px at 745 by 1040
    (an aftermath card as it is held, a Room read on its side at 1040 by
    744), real against ours:

    | | Printed | Ours |
    |---|---|---|
    | Aftermath, upright half: name bar fill | 53–108 | 54–106 |
    | art | 115–350 | 113–353 |
    | type bar fill | 359–408 | 361–405 |
    | text box | 422–555 | 420–555 |
    | Aftermath, half on its side, read: name bar fill | 56–113 | 56–110 |
    | art | 122–336 | 124–334 |
    | type bar fill | 346–395 | 348–393 |
    | text box | 408–688 | 409–686 |
    | its text box along the card | 584–950 | 585–946 |
    | Room, read: name bar fill | 37–92 | 38–90 |
    | art's top | 100 | 97 |
    | type bar, rim and all | 396–447 | 395–446 |
    | reminder band | 448–512 | 449–510 |
    | text box, rim and all | 515–712 | 516–712 |
    | gap between the doors | 534–551 | 535–551 |

    Text in either tops out at 9pt, as the printed halves' lines stand
    32–35 px apart.
- **Battle** (CR 310): not a declared layout. A face whose type line says
  Battle prints on a card turned on its side, as Invasion of Ikoria,
  Gobakhan, Tarkir, Zendikar and Alara print theirs: read with the card
  turned a quarter clockwise, the same way as a split card, its foot (and
  the footer) on the left. The art is the frame, edge to edge, fitted to
  cover the window between the name bar and the type line as Scryfall's
  art_crop of a battle is, and carried on mirrored behind the bars and the
  text box, which stand across the art's foot with a strip of it either
  side. The defense is in a shield on the text box's bottom right corner,
  Mockery's drawing of printed battles': a dark square burst into eight
  points, at its corners and its sides' middles, under a light rim. A
  Siege's front has a tab bent out of the box's right side, 53% down it,
  with its back's power and toughness beside it in grey at the text's size
  (CR 712.2c); its back is an ordinary card. The text tops out at 8.25pt,
  Zendikar's 32.5 px line pitch at 1040 px.
  - Every battle needs `defense`, which its corner holds alone (CR 310.4a),
    so it is no creature or planeswalker too; its text is plain lines.
  - A Siege is cast transformed when it is defeated (CR 310.12b), so it is
    refused anywhere but the front of `layout = "transform"`. A battle of
    no type, or another type, may stand alone or be either face of a
    double-faced card (CR 310.8, 310.9a). No such card has been printed:
    all 36 battles on Scryfall (`t:battle`) are Sieges, so its frame is the
    Siege's without the tab.
  - A battle is never a split half and never shares its box with an
    Adventure or a prepare spell: it prints sideways on a card to itself.
  - Orientation: every output is the card as it is held, 63 × 88mm, as
    Scryfall's 745 × 1040 battle images are. A PNG and a PDF page are
    portrait with the front drawn sideways; a sheet slot is an ordinary
    card's, so it cuts at card size; Curator's preview is that PNG, as for
    a split card. Turning the image instead would give the front a size of
    its own in every output and a slot no sheet grid has.

- **Adventure** (CR 715), **Omen** (CR 720) and **prepare** (CR 722): a card
  with a spell drawn inside its text box, `layout = "adventure"` or
  `"prepare"` with the spell
  in `[spell]`. The table is `[spell]` for both because the CR names both
  the same way, an inset frame holding a spell's alternative characteristics
  (CR 715.2, 722.2a), and because neither is a face: the card has one image
  and one set of normal characteristics (CR 715.2c, 722.2c), so `[back]`
  would wrongly promise a card back and `[right]` a split card's half. The
  top level stays the card, as with every other layout. One image, printed
  as Scryfall's images of Bonecrusher Giant, Murderous Rider, Brazen
  Borrower, Lovestruck Beast, Giant Killer and Gingerbread Hunter, and of
  Adventurous Eater, Blazing Firesinger, Abigale, Poet Laureate, Emeritus of
  Ideation and Lluwen, Exchange Student (Secrets of Strixhaven, `sos`, whose
  Scryfall layout is `prepare`) show:
  - An Adventure opens the text box like a book (CR 715.1): its left page is
    the Adventure, under a name bar with its cost and a type bar, each 3.8mm
    (45 of 1040 px) across the page's top in the spell's own colour with white
    type, the name bar lighter than the type bar, as Stomp's red, Swift End's
    black and Chop Down's warm grey are; a spell of two colours blends them,
    as Bring Back's does. The card's own text is the right page. The pages
    meet at the box's middle, each shaded toward the spine.
  - A prepare spell stands in a frame inset on the right of the box
    (CR 722.1–722.2), the card's text on the left. Its name and type sit on
    bars of the card's own shape, flat against the card's text and round at
    the frame's edge, their fills 3.55mm (42 px) deep and 0.7mm apart, in the
    spell's colour with white type, or silver for a spell of two colours as
    Heroic Stanza's and Pest Friend's are; under them the spell's own box, a
    shade darker than the card's, stops 3.25mm above the card's box's foot,
    leaving the frame for the power and toughness plate.
  - Reminder text goes where Oracle writes it, so it needs no place of its
    own: an Adventure's "(Then exile this card…)" closes the spell's text on
    the left page, and a prepare card's "(While it's prepared…)" follows
    "enters prepared" or "becomes prepared" in the card's own text.
  - Text in a half box tops out at 8.5pt, the size of the lines on the
    printed halves that have room to spare: Lovestruck Beast's left page and
    Adventurous Eater's and Blazing Firesinger's card text stand 32-34 px
    apart, and ours come out 32 and 33.5 px. Bonecrusher Giant's pages,
    printed at 31 and 27 px, come out 34 and 29 px, Libertinus being the
    narrower face so Stomp wraps to fewer lines.
  - The pitches and positions, fractions of the card's height at 745 by
    1040 px:

    | | Spell name bar | Spell type bar | Spell's own box | Card's box |
    |---|---|---|---|---|
    | Printed Adventure (Bonecrusher Giant) | 62.1–67.2% | 67.2–71.8% | page to 92.9% | 62.6–92.9% |
    | Ours | 62.6–66.8% | 66.8–71.2% | page to 92.3% | 62.6–92.6% |
    | Printed prepare (Adventurous Eater) | 63.3–67.3% | 68.1–72.1% | 73.1–88.6% | 62.9–92.2% |
    | Ours | 63.2–67.0% | 68.0–71.8% | 73.0–88.7% | 62.5–92.6% |

  - An Omen (CR 720, Tarkir: Dragonstorm) is `layout = "adventure"` with a
    `[spell]` whose type line has the Omen subtype, as Scryfall gives every
    one of the 13 printed omen cards layout `adventure`: the rules print the
    two alike (CR 715.1–715.2, 720.1–720.2) and the spell type is what tells
    them apart, so pasting Scryfall's layout and Oracle's `Instant — Omen` is
    enough. It is drawn as Marang River Regent, Twinmaw Stormbrood, Sagu
    Wildling, Scavenger Regent and Dirgur Island Dragon print it, not as an
    Adventure's book: a frame in the spell's colour inset on the left of the
    box, its name and type on bars of the card's shape, the name bar light
    and the type bar deep in the spell's colour with white type, over a box
    of its own tinted with that colour that runs to the foot of the card's;
    the card's own text is the right half. The halves meet at the box's
    middle. Its "(Then shuffle this card…)" closes the spell's text, as an
    Adventure's reminder does. Ours puts each bar about where print does
    (`omen-text` in `card.typ`); a spell of two colours, which no printed
    Omen is, gets blended bars over a box tinted with the blend.

  What a card with a spell inset must be, each refused by name otherwise:
  - The spell is an instant or a sorcery (CR 715.2, 720.2, 722.3c: a copy of
    it is cast), an Adventure's has the Adventure subtype and an Omen's the
    Omen subtype (CR 205.3k), as every printed one's does, so pasting
    Oracle's `Sorcery — Adventure` or `Instant — Omen` is enough. A spell
    with neither under `layout = "adventure"`, or with both, is refused. A
    prepare spell with the Adventure or Omen subtype is pointed at `layout =
    "adventure"`.
  - The spell has a cost: it is cast for it, and every printed one of the
    186 has one.
  - The spell has no `art`, `artist` or `color_indicator`: it is drawn in the
    card's text box under the card's art and artist, and no printed one has
    an indicator, so any is a mistake.
  - Neither half is a Saga, Class, planeswalker or station layout: the two
    share one text box and no frame has room for both.
  - Only a permanent has a prepare spell: prepared is a permanent's
    designation (CR 722.3a), so an instant or sorcery card with one is
    refused. An adventurer may be any card, as Twice Upon a Time, a sorcery,
    and the Virtues, enchantments, are.
  - `[spell]` with `[back]` or `[right]`, `[spell]` without a layout, an
    adventure or prepare layout without `[spell]`, and the spell under
    `[back]` (it has no card back) are refused as the other layouts' are.

  Searched on Scryfall (`layout:adventure`, 167 cards, of which 150 are
  Adventures and 17 Omens; `set:sos layout:prepare`, 36) on 2026-10-21. No
  printed spell half carries flavor text, so none of ours does in the test
  cards, though the format allows it.

- **Flip** (CR 710), `layout = "flip"` with the flipped half in
  `[flipped]`: not `[back]`, as a flip card's back is a card back
  (CR 710.1). One image, as Scryfall's `layout:flip` cards print: the
  upright half's name bar, text box and type line over the art, and the
  flipped half's below it, the same three upside down, read by turning the
  card round (CR 710.1a–b). Of the 45 printings on Scryfall (`layout:flip`,
  2026-10-21) all are old-frame Kamigawa cards and reprints except Budoka
  Gardener (Commander 2018) and Nezumi Graverobber (Commander Anthology
  Volume II) and Wilds of Eldraine's Role tokens, so the frame is measured on
  those two. Each half's power and toughness stand on its type line's
  right end, as they do on both. The card's colour is the upright half's,
  which Mockery paints both halves in.
  - The flipped half has no `cost` or `color_indicator`: a flip card's
    mana cost and colour don't change when it flips (CR 710.1c), and no
    printed flipped half has either. It has no `art` or `artist`: the art
    is one picture, the upright half's.
  - Both halves are permanents: only a permanent on the battlefield flips
    (CR 710.2). Both are plain lines with power and toughness or nothing on
    the type line's end, no Saga, Class, planeswalker or station frame and
    no loyalty or defense; a battle is refused as anywhere but its own card.
  - Their parts, in px at 745 by 1040 down a column clear of text, Budoka
    Gardener against ours (Nezumi Graverobber agrees to a pixel or two):

    | | Printed | Ours |
    |---|---|---|
    | upright name bar, rim and all | 34–98 | 33–100 |
    | upright text box | 99–235 | 101–232 |
    | upright type bar | 241–299 | 240–302 |
    | art | 308–648 | 309–648 |
    | flipped type bar | 653–715 | 653–715 |
    | flipped text box | 723–861 | 719–860 |
    | flipped name bar | 868–925 | 867–926 |
    | frame's foot | 948 | 947 |

    Text tops out at 8pt: the printed halves' lines stand 32.5 px apart,
    and ours come out 33–34.
- **Meld** (CR 712.4, 701.42), `layout = "meld"`: one file holds the pair,
  the card at the top level, the other in `[counterpart]` (CR 712.4a's
  word) and the card their backs make in `[melded]`. One file a card, each
  back naming the melded card and its half, was the other way, and was
  turned down: both files would carry the melded card, free to disagree,
  and either could be printed without the other. A pair is never half a
  pair here: a `"meld"` file with no `[counterpart]` or no `[melded]` is
  refused. Each printed card is still one card on a sheet: the file prints
  four card images, front then back, the second card's after, so a sheet
  gives each its slot at card size, back beside front, and `mockery render`
  writes `x.png`, `x.back.png`, `x.counterpart.png` and
  `x.counterpart.back.png`.
  - Which card is the top level is the print's: the top half of the melded
    card is on the back of the card with the ability that melds them, and
    the bottom half on its counterpart's, as the Eldritch Moon release
    notes say of Gisela and Bruna and The Brothers' War's of Titania and
    Argoth, and as Scryfall numbers every melded card after the
    counterpart (Brisela 15b after Bruna's 15, Urza, Planeswalker 238b after
    The Mightstone and Weakstone). So the top-level card needs a line that
    melds them into the `[melded]` card by name, as all seven printed pairs'
    have ("…then meld them into Brisela, Voice of Nightmares."), and a file
    whose `[counterpart]` has it instead is told to swap them.
  - The melded card has no cost: its mana value is its fronts' together
    (CR 202.3c, 712.8g). It may have a `color_indicator`, as The Brothers'
    War's do; Eldritch Moon's are colourless. All three parts are
    permanents, as melding puts the pair onto the battlefield (CR 712.4a,
    701.42a). No battle is part of one.
  - Each front prints as printed fronts do (Gisela, Bruna, Urza, Lord
    Protector, The Mightstone and Weakstone, Phyrexian Dragon Engine): a
    meld mark on the name bar's left, Mockery's own, a dark disc with two
    points meeting in it; and when the melded card is a creature, its power
    and toughness in grey beside the front's own, as Gisela's and Bruna's
    "9/10" and Dragon Engine's "9/9" are. The counterpart's "(Melds with
    …)" is Oracle text, so it goes in its `rules` like any reminder.
  - The melded card is an oversized card face (CR 712.4), two cards on
    their sides, 88 by 126mm, as Scryfall's 1040 by 1490 image of Urza,
    Planeswalker is. Printed ones are a card's frame grown to that size,
    text and all, so Mockery draws the melded face a card wide and as tall
    as that shape, 90.2mm, and scales it by 88/63. The 2.2mm a card lacks
    goes where Brisela has it: its art and type line stand lower than a
    grown card's, its text box's foot where a card's is, and its black band
    deeper. Each back holds its half of that, read with the card turned a
    quarter anticlockwise, the other way from a split card, the cut along its long edge with
    its corners rounded, the notch Scryfall's images show at the seam.
    Against Brisela, the two backs stitched at 1040 by 1490 (Mishra, Lost to
    Phyrexia agrees within a few px):

    | | Printed | Ours |
    |---|---|---|
    | type bar, rim and all | 828–905 | 835–915 |
    | text box's foot | 1335 | 1335 |
    | power and toughness plate | 1310–1385 | 1300–1375 |
    | footer's baseline | 1395 | 1430 |

    Which way: Eldritch Moon's backs turn the other way from a split card,
    as those who scanned the six printed backs report
    (slightlymagic.net, "ELDRITCH MOON", 2016: flipping a split card over,
    "you turn your head 90° counterclockwise to view", a meld card's
    "90° clockwise"). Not measured here on a physical card.

Insight reads every face's rules text, each as its own face (CR 712.8,
709.3b), and an inset spell's as the face it is while cast (CR 715.3b). Matched against Scryfall's images of Fable of the Mirror-Breaker,
Delver of Secrets, Emeria's Call, Turntimber Symbiosis, Valki, God of Lies,
Kazandu Mammoth, Agadeem's Awakening, Far // Away, Turn // Burn, Wear //
Tear, Cut // Ribbons, Never // Return, Spring // Mind, Commit // Memory,
Ticket Booth // Tunnel of Hate, Bottomless Pool // Locker Room, Dazzling
Theater // Prop Room, Smoky Lounge // Misty Salon, Roaring Furnace //
Steaming Sauna and Restricted Office // Lecture Hall, with their Oracle text rendered through Mockery beside them. The face
marks, the hint bar and the fuse bar are Mockery's own drawing, not Wizards'
symbols
([ADR-0039](../adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).

Curator's form edits every part: a picker above the form chooses the front
or the back (left or right half, the card or its Adventure or prepare
spell, the upright or flipped half, or a meld pair's card, counterpart or
melded card), and writing one never touches the others or the layout. The
preview has a picker of the card images too, for a double-faced card's two
and a meld pair's four, and picking a part shows the image it is drawn on.

### Layouts, from what the text says

`rules` stays Oracle text: paste a card's Oracle text in and it takes its
layout from the markers in it, never from a list of frames
(`core/src/rules.rs`). The template gets the text already read into one of:

- **Plain**: one paragraph after another, art above, text below.
- **Saga**, when a line opens with chapter numerals, `I — …` or `II, III —
  …` (CR 714.2): the art stands down the right and the type line goes to the
  foot (CR 714.1). The text column holds what comes before the chapters (the
  reminder, read ahead) across its top, then a section a chapter ability with
  its numerals as hexagons straddling the column's left edge, as printed
  Sagas' do, the sections sharing the height left over. They used to sit in
  a strip of their own inside the column, which cost the text 3 mm of its
  26 mm width. Lines after the last chapter are a Saga creature's other
  abilities, set with its power and toughness in a box below the type line
  (CR 714.1a). The chapters must count up from I with none missed and none
  out of place.
- **Class**, when a line is a level bar, `{2}{U}: Level 2` (CR 716.2): the
  art stands down the left, the type line at the foot (CR 716.1). The level 1
  section comes first, its reminder set smaller and ruled off; then each level
  bar, its cost on the left and "Level N" on the right, over its abilities.
  Levels count up from 2.
- **Planeswalker**, when a planeswalker's line opens with a loyalty cost,
  `+1: …`, `−2: …`, `0: …` or `−X: …` (CR 606.2), typed with Oracle's minus
  sign, a hyphen or an en dash and printed with the minus sign. A frame of its
  own, in the proportions measured on printed modern-frame planeswalkers
  (Teferi, Hero of Dominaria; Karn, the Great Creator; Liliana of the Veil
  from DMU; Ugin from M21; Chandra, Torch of Defiance; Minsc & Boo): an
  art window whose sides bow out, narrowest at its rounded corners and widest
  at mid-height, inside a band of the bars' rim colour that is pinched into a
  neck under each corner and flares into the name bar above and the type
  line below, then the type line, and a text box
  narrower than the frame in a band of its own that flares out of the type
  line, rounds its foot's left corner, arches over the middle of the card's
  foot and flares into the loyalty shield over the box's right corner, the
  art running on behind the box under a pale wash. The art is framed as
  printed: Scryfall's `art_crop` of each of those six lands on the card at its
  own size, as wide as the window at its widest and from the window's top to
  its foot, so a user's art is fitted to the window's width and hung from its
  top, cropped only when it would not fill the window, and carries on
  mirrored behind the box where it runs out. Three abilities or fewer keep a window near a
  creature's, the type line at 59% of the card's height as printed; four or
  more take the printed tall variant, the window shorter and the type line at
  52%. Static abilities run across the box; each loyalty ability is a band of
  its own, shaded in turn, never shallower than its badge with a margin, the
  bands sharing the height left over. Its cost sits on a badge standing out
  over the frame left of the box, 6 by 3.7mm as printed ones are, with a
  colon after it. The badges are Mockery's own shapes: a dark plate with a
  light rim whose top rises to a point for a cost that adds loyalty, whose
  foot drops to one for a cost that removes it, and flat for `0`. The loyalty
  is on a dark shield under a thin light rim, its top low at the corners,
  rising to a sharp horn either side of the middle and scooped round between
  them, its sides dented in as they fall to a foot that runs to a point:
  Mockery's drawing of the printed shield, measured on the six walkers above,
  which agree to a pixel, at 107 by 67 px of 745 by 1040 with its horns 35 px
  over the box's foot. A cost of `+0` or `−0` is refused:
  Oracle writes it `0`. Loyalty costs on a card that is not a planeswalker
  stay plain lines: a loyalty ability there is legal ("normally, only
  planeswalkers have loyalty abilities", CR 606.2), and a frame with no
  badges prints it as the activated ability it is.
- **Station**, when a line opens `3+ | …` (CR 721.2): the standard layout,
  with each threshold a striation of the text box a step darker than the one
  above, its number on a disc at the box's left edge, and the lines after it
  belonging to it. What comes before the first threshold holds at all times,
  the station keyword among it. The last striation runs to the foot of the box
  and holds the power and toughness. Thresholds must climb.

A text with the markers of more than one of these is refused, and so are
chapters on a card whose type line has no Saga and level bars on one with no
Class: they print only on that frame (CR 714.1, 716.1), and either on another
card is a type line missing its subtype far more often than a card meant that
way, as a power and toughness on a noncreature is. Station symbols need no
type: a station card is any card printed with them (CR 702.184b). These follow
where printed cards put chapters, levels and thresholds; the shapes, colours
and fonts are Mockery's own frame
([ADR-0039](../adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).
They were matched by rendering real cards' Oracle text next to their Scryfall
images: The Eldest Reborn, History of Benalia and Summon: Bahamut; Wizard and
Ranger Class; Exploration Broodship, Uthros Research Craft and The Eternity
Elevator; Khalni Garden, Overgrown Tomb and Dryad Arbor; Liliana of the
Veil, Teferi, Hero of Dominaria, Chandra, Torch of Defiance, Karn, the Great
Creator, Gideon Blackblade, Ugin, the Spirit Dragon and Minsc & Boo,
Timeless Heroes (2026-10-21).

### How big the text comes out

Text in a striated layout is measured as the size most of a card's glyphs
are set at (`core/tests/sizes.rs`, which takes any cards through
`MOCKERY_SIZES`), over real cards' Oracle text rendered through Mockery, so a
column's proportions are judged by the size printed text comes out at rather
than by a test card's own length. Before and after the chapter symbols moved
onto the column's edge (2026-10-21):

| Card | Before | After |
|---|---|---|
| Fable of the Mirror-Breaker | 7.75pt | 7.75pt |
| The Eldest Reborn | 8pt | 8.5pt |
| The Akroan War | 7.75pt | 8pt |
| Urza's Saga | 7.25pt | 7.5pt |
| Summon: Bahamut | 7pt | 7.25pt |
| Wizard, Ranger, Bard, Druid, Barbarian Class | 9, 8.25, 8, 7.5, 8.5pt | unchanged |
| The Cleansing of Shinovar (test card) | 6.25pt | 6.75pt |
| Jedi Knight Class (test card) | 8pt | 8pt |

Set beside Scryfall's images at the same scale, every real Saga and Class
above already comes out at or a little above its printed size, Libertinus
having a smaller x-height than the printed face. The Cleansing of Shinovar is
small because its text is half again as long as Fable's and has flavor text,
which printed Sagas rarely carry, not because the column is narrow; the
Classes' columns are already wider than printed ones, so they were left as
they are. Fable keeps its 7.75pt because its last chapter now holds the
back's power and toughness. A test holds the striated test cards above a
floor.

Planeswalkers were measured the same way, then put in their printed
proportions. Fractions of the card's height, the real cards' read off
Scryfall's images, ours off renders at the same scale:

| | Art window | Type line | Text box |
|---|---|---|---|
| Printed, 3 abilities | 9.5–55.5% | 56.5–62% | 62.5–91% |
| Ours before | 11.5–53.5% | 54.5–59.5% | 60–95% |
| Ours, 3 abilities | 11.5–55% | 56–61% | 62–91% |
| Printed, 4 abilities | 9.5–48.5% | 49.5–55% | 55.5–91% |
| Ours, 4 abilities | 11.5–48.5% | 49.5–54.5% | 55.5–91% |

The window's top stays at 11.5%: the name bar is every card's. The box is
shorter and narrower than it was, as printed ones are, so the sizes fell:
Teferi 8.25 to 6.75pt, Ugin 8.5 to 7pt, Chandra 7.5 to 6.5pt, Karn 6.75 to
6pt, Liliana of the Veil 9 to 7pt, Minsc & Boo 6pt either way. In line pitch
that is the printed cards' or a little more (Teferi's lines 2.46mm apart in
print and in ours), Libertinus being the smaller face at a given size.

The window's shape was measured on Teferi and Chandra, whose coloured edges
can be told from their art, as fractions of the card's width from its edge.
Its top and bottom necks sit just under the bars, its corners just inside
them; ours are at the same heights relative to our window, which starts
lower.

| | Window side, corners | Window side, mid-height | Band's outer edge, mid | Top neck | Foot neck |
|---|---|---|---|---|---|
| Printed | 9–10% | 7% | 5.5% | 7.5% | 7% |
| Ours | 9.9–10.5% | 7% | 5.6% | 7% | 6.6% |

The band is the bars' rim, in the card's colours (Teferi's white running into
blue), and flows out of the bars' feet into the necks as printed bands do.
Measured again on Teferi at 745 px wide once our bars took the printed
shape, the necks' outer lines stand some 47-50 px from the edges, nearer
than the 7.5% and 7% above, and ours were moved out to 7% and 6.6% to meet
them. The tall four-ability frame takes the same shape over its shorter
window.

The text box was measured the same way on Teferi, Chandra and Liliana of the
Veil: its outline 11.7% of the width from the left edge and 7.2% from the
right, as ours was already; its foot at 91.4% of the height, the band under
it 0.8% deep with its foot's outer left corner some 4mm round; and an arch
in the middle of the foot, 15% of the width across and rising 1% of the
height, where printed cards set their holofoil stamp. Ours has the arch
with nothing in it. The band's sides stand 5.8% from the edges, and the
shield stands over the box's corner with the band flaring into its shoulder
and out of its side, where the box sat in a cut-out collar before.

### The name and type bars

Every card's name bar and type line take printed modern frames' bar, the
shape of Lotus Cobra's and every M15 frame's, as Mockery's own drawing
(`plate` in `core/src/card.typ`): wider than the art and the text box, each
end a half ellipse bulging out past them, a rim in the card's colour between
two dark lines (a gold card's in its two colours, left to right, a land's in
the colours it is or makes), and a fill lighter at the top than the foot over
a shadow along its foot and left end, so the bar reads as raised. A gold card
and a colourless land fill theirs in the frame's own tone, as printed ones
do; other cards keep a pale fill. Name and type bar share the shape; since
*The card's height* below, their fills are 4.4 and 4.55mm deep, as measured,
and a planeswalker's name bar 4.05mm. A fused split half keeps its pointed inner end and
takes the round end outside. Measured at 745 × 1040 px on Lotus Cobra,
Kolaghan's Command, Wurmcoil Engine, Fabled Passage, Fable of the
Mirror-Breaker, Wizard Class and Teferi, Hero of Dominaria:

| | Outer edge from card edge | Inner dark line from edge | End's depth | Height | Rim |
|---|---|---|---|---|---|
| Printed | 33-35 px, 4.5-4.7% | 42 px | ~14 px | 72-75 px, 6.9-7.2% | 6 px |
| Ours before | 48 px, 6.4% (one line, no rim) | 48 px | ~5 px (1mm corner) | 62-64 px, 6.0-6.2% | none |
| Ours | 33-34 px, 4.5% | 42 px | ~13 px | 70-75 px, 6.7-7.2% | 6 px |

The text sizes did not move: every test card and walker is set where it was.

### The card's height

Every face stands where printed M15-frame cards put their parts, measured as
fractions of the card's height at 745 × 1040 px on Lotus Cobra, Lightning
Helix, Llanowar Elves, Counterspell, Wurmcoil Engine, Kolaghan's Command,
Fabled Passage and Emeria's Call, which agree to a pixel or two, then Teferi,
Hero of Dominaria and Chandra, Torch of Defiance for planeswalkers, Fable of
the Mirror-Breaker and Wizard Class for Sagas and Classes, and Wear // Tear
(read on its side, fractions of its 63mm) for split cards. A bar's figures
are its fill's; its rim stands round it. `layout-of` in `core/src/card.typ`
holds them.

| | Name bar | Art | Type bar | Text box | Frame stops | P/T plate |
|---|---|---|---|---|---|---|
| Printed | 4.6–9.6% | 10.9–56.2% | 56.4–61.5% | 62.6–92.5% | 92.9% | 89.0–94.7% |
| Ours before | 4.7–10.1% | 11.5–52.9% | 53.9–58.4% | 59.3–96.2% | 96.9% | inside the box, 91–95.5% |
| Ours | 4.6–9.6% | 10.9–56.3% | 56.4–61.5% | 62.6–92.5% | 92.9% | 88.9–94.4% |
| Printed walker, 3 abilities | 3.9–8.5% | window 9.9–55.4% | 56.0–61.2% | 62.4–91.3% | 92.2% | |
| Ours before | 4.7–10.1% | window 11.5–55% | 56–61% | 62–91% | 96.9% | |
| Ours | 3.9–8.5% | window 9.9–55.4% | 56.4–61.6% | 62.6–91.4% | 92.2% | |
| Printed walker, 4 abilities | 3.9–8.5% | window 9.8–48.4% | 49.2–54.4% | 55.7–91.3% | 92.1% | |
| Ours | 3.9–8.5% | window 9.9–48.4% | 49.4–54.6% | 55.7–91.4% | 92.2% | |
| Printed Saga, Class | 4.6–9.6% | columns 10.9–83.6% | 84.4–89.6% | | 91.7–91.9% | |
| Ours before | 4.7–10.1% | columns 11.5–85% | 85.6–90.6% | | 96.9% | |
| Ours | 4.6–9.6% | columns 10.9–83.6% | 84.5–89.6% | | 91.8% | |
| Printed split half | 7.1–13.7% | 14.8–53.2% | 54.5–58.9% | 60.7–89.5% | fuse foot 94.8% | |
| Ours before | 7–13.6% | 14–46.2% | 46.9–54% | 54.9–86.4% | fuse foot 95.6% | |
| Ours | 7.1–13.8% | 14.8–53.2% | 54.4–58.9% | 60.6–88.8% | fuse foot 94.6% | |
| Printed battle front, on its side | 4.8–11.8% | frame 3.8–96.4% | 58.3–65.2% | 66.8–96.1% | 96.4% | shield centre 92.5% down, 94.0% across |
| Ours before | portrait card | | | | | defense in the P/T plate |
| Ours | 4.8–11.8% | frame 3.8–96.3% | 58.3–65.3% | 66.9–96.2% | 96.3% | shield centre 91.8% down, 94.0% across |

What changed with it:

- The art is taller and the type line lower, so the text box is 26.3mm deep
  where it was 32.
- The frame stops just under the text box, and a black band, 7% of the card,
  holds the footer where a printed card's collector line is: the artist on
  the left and "Unofficial fan content · not a Magic card" on the right, at
  4.4pt, its baseline 3mm off the card's edge. A split card's runs along the
  card's foot, which read on its side is its left.
- The power and toughness sit on a raised plate in the bars' colours, its
  corners cut, 10.15 × 4.9mm as Cobra's is, straddling the text box's bottom
  right corner: 0.85mm out past its side and 1.9mm below its foot. The
  loyalty shield and the defense box are as they were.
- A planeswalker's name bar stands higher and shallower than other cards',
  as printed ones do, so its window starts at 9.9% where ours started at
  11.5%; the window's foot stayed where it was. The frame stops where the
  band round the text box sweeps into the black band, from the frame's edge
  8mm above the band's foot round to 9.5mm in, as Teferi's does, and on the
  right into the band's side over the shield; the box's own foot rounds off
  4mm on the left.
- A modal face's hint bar is Emeria's Call's: 3.1mm deep and at least 43% of
  the card across, its type at the left and its cost at the right, over the
  text box's foot with its left end at the frame's edge.
- A split half's type bar is shallower and set at 6.5pt, as Wear // Tear's.

Rules text is spaced as printed. Measured with the same rows on Cobra, Helix,
Kolaghan's Command, Wurmcoil, Questing Beast, Gigantosaurus, Colossal
Dreadmaw and Teferi, a printed card's lines stand 2.15 x-heights apart at
any size, its paragraphs 0.5–1mm further, and its divider 2.1mm under the
rules text's last baseline and 1.6mm over the flavor text's capitals, a
hairline fading out at its ends. The largest printed rules text has an
x-height of 1.48mm (Cobra, Helix, Llanowar, Counterspell), which Libertinus,
with an x-height of 0.44em, reaches at 9.5pt, so text now fits from 9.5pt
rather than 9pt; Wear // Tear's tops out at 1.18mm, 7.5pt in a split half.
Ours had lines 2.42 x-heights apart, so the leading went from 0.42em to
0.3em. A short text is centred in the box at that size, as printed ones are,
rather than set at the top: the printed gaps do not grow with the room
(Cobra's rules-to-flavor pitch is Helix's, 5.7mm, though Helix fills its box
and Cobra does not), so neither do ours. A lone line is centred across the
box too, as Counterspell's is. The text runs to 1.2mm of the box's sides
where it stood 2mm in, near printed cards' 1mm.

Body sizes before and after (`core/tests/sizes.rs`; the printed figure is
the x-height read off Scryfall's image, as the Libertinus size with that
x-height). The size most glyphs are set at is the footer's on a card with
almost no text (Counterspell) and the fuse bar's on Wear // Tear.

| Card | Printed | Before | After |
|---|---|---|---|
| Lotus Cobra | 9.5pt | 9pt | 9.5pt |
| Lightning Helix | 9.75pt | 9pt | 8.75pt |
| Kolaghan's Command | 9.5pt | 9pt | 8.75pt |
| Llanowar Elves | 9.5pt | 9pt | 9.5pt |
| Wurmcoil Engine | 9.25pt | 9pt | 9.5pt |
| Gigantosaurus | 9.25pt | 9pt | 9.5pt |
| Colossal Dreadmaw | 9.25pt | 9pt | 9.5pt |
| Fabled Passage | | 9pt | 8.5pt |
| Questing Beast | 7pt | 7.25pt | 7pt |
| Emeria's Call | | 9pt | 9pt |
| Fable of the Mirror-Breaker | 8.25pt | 7.75pt | 8pt |
| Wizard Class | 8.75pt | 9pt | 9.5pt |
| Teferi, Hero of Dominaria | 7.25pt | 6.75pt | 7.5pt |
| Chandra, Torch of Defiance | | 6.5pt | 6.5pt |
| Karn, Liliana, Minsc & Boo, Ugin | | 6, 7, 6, 7pt | 6.25, 7, 6, 7.5pt |
| Bridge Four Windrunner (test card) | | 8pt | 7.5pt |
| Chunky Chasmfiend // The Shattered Plains | | 9, 9pt | 9.5, 9.5pt |
| Force Push // Force Pull | | 8.75pt | 7.5pt |
| Proofsheet Drone | | 7.5pt | 7.25pt |
| The Cleansing of Shinovar | | 6.75pt | 7pt |
| Jedi Knight Class | | 8pt | 8.5pt |
| Jasnah, Veristitalian Scholar | | 8.75pt | 9pt |
| Vindicator-Class Heavy Cruiser | | 5.25pt | 5pt, flavor dropped |

The cards that fill their box came out smaller (Helix, Kolaghan's Command,
Passage, Force Push) and the ones that do not came out at the printed size:
Libertinus at a printed x-height is wider than Wizards' face, so a full box
holds fewer words. The Vindicator test card's text, longer than any printed
Spacecraft's, no longer fits with its flavor line at 5pt, and printed
Spacecraft carry none, so the line went.

### Ability and flavor words

A line opening with words before " — " sets those words in italics, as
printed cards set ability words (CR 207.2c, `Landfall —`) and flavor words
(CR 207.2d, `Vicious Mockery —`, `Mega Flare —` in Summon: Bahamut's fourth
chapter). The CR lists ability words but no flavor words, which each set
coins, and corrupted, printed in italics, is in neither list, so the rule is
the position rather than a list (`ability_word` in `core/src/rules.rs`),
less what printed cards set upright there:

- a keyword the CR writes with a long dash, as `Companion —` and `Exhaust —`
  (CR 702.139a, 702.177a);
- a cost, anything with a symbol: `Prototype {2}{R} —`, spree's `+ {1} —`;
- a mode's name after a bullet, as Citadel Siege prints `• Khans —` upright;
- an instruction whose options follow the dash, `Choose one — …; or …` and
  `… faces a villainous choice — …` (CR 701.55a);
- a Saga's chapter numerals, and anything not opening with a letter, as a
  die roll's `1 —`.

Run over every Oracle text in Scryfall's bulk data, the words this sets in
italics are ability words, flavor words, and a handful of Un-set heads
(`Prize —`) whose printing was not checked. Matched against Lotus Cobra,
Insufferable Balladeer, Kain, Traitorous Dragoon, Vivisection Evangelist,
Lutri, the Spellchaser and Citadel Siege.

## Next

- Art cropping control (focus point), set symbol and rarity, collector line.
- ~~Double-faced and split layouts.~~ Done: transform, modal and split with
  fuse, aftermath and Rooms, adventure and prepare, flip and meld, see
  *Cards of two parts*. Not yet: omen. ~~A
  planeswalker's loyalty abilities laid out as printed.~~ Done, see
  *Planeswalker* under *Layouts*.
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
