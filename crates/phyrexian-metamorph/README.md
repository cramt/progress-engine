# Phyrexian Metamorph

Proxies of Magic cards, as a PDF to print and cut. Binary `metamorph`.

Two crates:

- `metamorph-sheet` lays JPEGs out at 63×88 mm on the page, with cut marks.
  It knows the size of a card and nothing else about Magic, and does no I/O,
  so it builds for wasm as it is.
- `metamorph-cli` decides what goes on the sheet and fetches it from Scryfall:
  one `POST /cards/collection` per 75 cards, then the images, eight at a time.

## A list of printings

The core of the tool. Each printing is `set/num`, as a `.deck.toml` writes it;
name one twice for two copies.

```
metamorph printings mh3/154 neo/266 mid/51 mid/51 -o proxies.pdf
```

## A deck

The same list, worked out from a `.deck.toml`: every card that needs a physical
copy, which leaves out the maybeboard and anything set aside. Each line prints
in the printing it names; a line by name gets Scryfall's default printing.

```
metamorph deck decks/insider-trading.deck.toml -o proxies.pdf
metamorph deck decks/insider-trading.deck.toml --no-basics -o proxies.pdf
metamorph deck decks/insider-trading.deck.toml --no-basics --minus collection.toml -o proxies.pdf
```

`--minus` leaves out what the collection owns. The count comes from
`chip-decklist`'s `supply`, the same call as Curator's Sourcing view
(ADR-0036), so a copy in another printing or finish counts as owned, and so
does a copy sleeved in another deck. A run prints what it left out, and
stdout lists what went on the sheet, one line per card.

## The sheet

| Flag | Default | |
|---|---|---|
| `--paper` | `a4` | `a4`, `a3`, `letter`, `legal`, portrait. A4 and Letter fit 9 cards, A3 16. |
| `--gap` | `0` | Millimetres between cards. With none, one cut separates two cards. |
| `--margin` | `5` | Millimetres kept clear at the paper's edge. The grid is centred inside it. |
| `--[no-]cut-marks` | on | Marks in the margin along every card edge, 1 mm clear of the grid. |
| `--image` | `large` | Scryfall's `large` (672×936, about 270 dpi at card size), `normal` or `border-crop`. `png` is not offered: the sheet embeds JPEG bytes as they are, and a PNG would have to be decoded. |
| `--sides` | `both` | A card with two sides gets a proxy of each side, or with `front`, of the front only. |

An image is embedded once however many copies print, so four of a basic land
cost one image.

## Measured

`decks/insider-trading.deck.toml` from cramt/mtg, release build, 2026-10-17:

| Run | Proxies | Pages (A4) | PDF | Wall |
|---|---|---|---|---|
| whole deck | 105 (100 cards, 5 with two sides) | 12 | 11.3 MB | 3.1 s |
| `--no-basics` | 77 | 9 | 11.2 MB | |
| `--no-basics --minus collection.toml` | 68 (11 basics, 26 owned left out) | 8 | 10.1 MB | |

Fetching images one at a time took 26 s for the whole deck. Eight workers
brought it to 3.1 s.
