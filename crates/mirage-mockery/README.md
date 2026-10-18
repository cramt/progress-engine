# Mirage Mockery

Custom Magic cards, from a `.card.toml` to a PNG or a PDF, and on sheets to
print and cut. Three crates:

- `mockery-core`: the card format, the real-card refusal, Insight's reading
  and the Typst renderer. No network and no argument parsing, so it builds for
  wasm as it is.
- `mockery-cli`: binary `mockery`.
- `mockery-wasm`: `mockery-core` for Curator's custom cards page.

The mana symbols are Scryfall's, committed in `core/symbols/` and compiled
in; `core/symbols/fetch.sh` refreshes them when a set adds one.

```
mockery render my-card.card.toml                  # my-card.png beside it, 300 ppi
mockery render a.card.toml b.card.toml -o set.pdf # one card a page
mockery sheet *.card.toml -o sheet.pdf --paper letter --gap 2  # Metamorph's flags
mockery check *.card.toml                         # no render: the refusal and Insight's reading
```

Every command checks the cards against the card index first (`gauntlet sync`
builds it, or pass `--index decks/index.jsonl`):

- **A real card is refused.** That covers a name Magic already has, and the
  same cost and rules text under another name. Real cards are Phyrexian
  Metamorph's to print
  ([ADR-0039](../../docs/adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).
- **Insight reads the rules text**, and reports each line it could not read
  and the word it stopped at. A warning only.

The card format, the layout and what comes next are in
[docs/research/mirage-mockery.md](../../docs/research/mirage-mockery.md). An
example card is in `core/tests/cards/`.

## In Curator

Meldweb Curator's `/cards` page edits `cards/*.card.toml` in the Magic repo
beside the card drawn from it, and prints a sheet of them. That is the third
crate, `mockery-wasm`, its own module so no other page downloads Typst. With
no card index in the browser, the refusal asks Scryfall's search for a card by
the same name and for the cards at the same cost holding the card's longest
word, then compares with the same `real_card` the CLI uses
([ADR-0040](../../docs/adr/0040-curator-prints-proxies-and-custom-cards-in-the-browser.md)).

Mirage Mockery is unofficial Fan Content permitted under the Fan Content
Policy. Not approved/endorsed by Wizards. Portions of the materials used are
property of Wizards of the Coast. ©Wizards of the Coast LLC. It is free and
non-commercial, and every sheet it prints carries this notice.
