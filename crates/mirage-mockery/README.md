# Mirage Mockery

Custom Magic cards, from a `.card.toml` to a PNG or a PDF, and on sheets to
print and cut. Three crates:

- `mockery-core`: the card format, Insight's reading
  and the Typst renderer. No network and no argument parsing, so it builds for
  wasm as it is.
- `mockery-cli`: binary `mockery`.
- `mockery-wasm`: `mockery-core` for Curator's custom cards page.

The mana symbols are Scryfall's, committed in `core/symbols/` and compiled
in; `core/symbols/fetch.sh` refreshes them when a set adds one.

```
mockery render my-card.card.toml                  # my-card.png beside it, 300 ppi
                                                  # (a double-faced card: and my-card.back.png)
mockery render a.card.toml b.card.toml -o set.pdf # one card a page
mockery sheet *.card.toml -o sheet.pdf --paper letter --gap 2  # Metamorph's flags
mockery check *.card.toml                         # no render: Insight's reading
```

Every command has Insight read the rules text first, and reports each line it
could not read and the word it stopped at. A warning only. Nothing checks
whether a card is a real one: that refusal was dropped, as more nuisance than
safeguard
([ADR-0039](../../docs/adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).

Every card's name bar and type line are printed modern frames' bar, in
Mockery's own drawing: wider than the art and text box, round D-shaped ends,
a rim in the card's colours and a raised fill. A face's parts stand where
printed M15-frame cards put theirs, measured as fractions of the card's
height: the type line at 56–62%, a text box of 26mm, the power and
toughness on a raised plate straddling the box's corner, and the frame
stopping just under the box over a black band that holds the footer.
Rules text fits from 9.5pt, a printed card's largest, and a short text is
centred in its box.

A card's layout comes from its Oracle text: chapter lines make a Saga, level
bars a Class, station thresholds a Spacecraft's striations, loyalty costs a
planeswalker's frame in printed walkers' proportions, its art window bowed
out at the sides like a barrel inside a band that necks under the name and
type bars and holding the art as printed walkers frame theirs, over a
narrower text box of badged bands that the art runs on behind, in a band that
arches over the card's foot and flares into a horned loyalty shield, each refused when
numbered out of order.
A face whose type line says Battle prints as printed battles do, on a card
turned on its side and read the way a split card is: its art filling the
frame, the type line and text box across its foot and its defense in a
shield on the box's corner, a Siege's front with its back's power and
toughness on a tab out of the box. Every battle needs `defense`, and a Siege
is refused anywhere but the front of `layout = "transform"`, as it is cast
transformed (CR 310.12b). The image is still the card's own size, the front
drawn sideways in it, so a sheet cuts it as any other.
Chapters on a card that is no Saga and level bars on one that is no Class are
refused too; loyalty costs on a card that is no planeswalker print as plain
lines. Ability and flavor words before a line's " — " are set in italics. A `color_indicator` colours a card with no cost, and a
colourless land is tinted by the mana it adds.

A face with no `art`, or whose art Curator does not have yet, prints a
placeholder drawn by DiceBear (`core/src/placeholder.rs`): one of its CC0
abstract styles (blobs, glass, shapes or waves), picked and seeded by the
card's name so the card always draws the same one, in the card's colours. The
Rust DiceBear core is patched to drop jsonschema's network features, which
cannot build for wasm (`vendor/README.md`).

A card of two parts declares it: `layout = "transform"` or `"modal"` with a
`[back]` face, `"split"` with a `[right]` half, or `"adventure"` or
`"prepare"` with the `[spell]` drawn in its text box. A double-faced card is two
card images, front and back: two pages of a PDF, `x.png` and `x.back.png`, or
two slots on a sheet, its back beside its front. A split card is one image,
turned on its side, and what its halves say picks its frame: halves that
both say Fuse print it once on a bar across the foot, shaped as the name and
type bars are; a right half with Aftermath lies on its side under the left,
which stands upright, as Cut // Ribbons prints; and two halves that are
both `Enchantment — Room` are a Room's doors, under one picture, one shared
type line and their shared unlock reminder on a band across the card.
Aftermath on the left half, aftermath with fuse or on anything but instants
and sorceries, and a Room whose halves' type lines differ are refused. An Adventure is the left-hand page of a text box opened like a book,
under its own name and type bars in its colour; a prepare spell stands in a
frame inset on the right of the box, its bars shaped as the card's. Either
is refused unless it is an instant or sorcery with a cost and plain text,
an Adventure with the Adventure subtype, and only a permanent is prepared.

The card format, the layouts and what comes next are in
[docs/research/mirage-mockery.md](../../docs/research/mirage-mockery.md).
Example cards, one per layout, are in `core/tests/cards/`.

## In Curator

Meldweb Curator's `/cards` page edits `cards/*.card.toml` in the Magic repo
beside the card drawn from it, and prints a sheet of them. A card is edited
as a form, with buttons for its cost (each symbol added where Oracle text
writes it, `core/src/cost.rs`), types, colour indicator and symbols, or as the TOML
itself (a card of two parts picks its face above the form, and a
double-faced one below the preview too), switching freely: the file is the only state, and `core/src/form.rs`
reads the form out of it and writes edits back in place, so comments and
order survive. That is the third crate, `mockery-wasm`, its own module so no
other page downloads Typst
([ADR-0040](../../docs/adr/0040-curator-prints-proxies-and-custom-cards-in-the-browser.md)).

Mirage Mockery is unofficial Fan Content permitted under the Fan Content
Policy. Not approved/endorsed by Wizards. Portions of the materials used are
property of Wizards of the Coast. ©Wizards of the Coast LLC. It is free and
non-commercial, and every sheet it prints carries this notice.
