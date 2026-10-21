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

A card's layout comes from its Oracle text: chapter lines make a Saga, level
bars a Class, station thresholds a Spacecraft's striations, loyalty costs a
planeswalker's frame in printed walkers' proportions, its rounded art window
over a narrower text box of badged bands, each refused when numbered out of
order.
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
`[back]` face, or `"split"` with a `[right]` half. A double-faced card is two
card images, front and back: two pages of a PDF, `x.png` and `x.back.png`, or
two slots on a sheet, its back beside its front. A split card is one image,
turned on its side, and halves that both say Fuse print it once across the
foot.

The card format, the layouts and what comes next are in
[docs/research/mirage-mockery.md](../../docs/research/mirage-mockery.md).
Example cards, one per layout, are in `core/tests/cards/`.

## In Curator

Meldweb Curator's `/cards` page edits `cards/*.card.toml` in the Magic repo
beside the card drawn from it, and prints a sheet of them. A card is edited
as a form, with buttons for its cost, types, colour indicator and symbols, or as the TOML
itself (a card of two parts picks its face above the form and below the
preview), switching freely: the file is the only state, and `core/src/form.rs`
reads the form out of it and writes edits back in place, so comments and
order survive. That is the third crate, `mockery-wasm`, its own module so no
other page downloads Typst
([ADR-0040](../../docs/adr/0040-curator-prints-proxies-and-custom-cards-in-the-browser.md)).

Mirage Mockery is unofficial Fan Content permitted under the Fan Content
Policy. Not approved/endorsed by Wizards. Portions of the materials used are
property of Wizards of the Coast. ©Wizards of the Coast LLC. It is free and
non-commercial, and every sheet it prints carries this notice.
