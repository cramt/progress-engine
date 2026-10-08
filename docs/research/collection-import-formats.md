# What other apps' collection exports look like

Question: a Meldweb Curator user keeps their collection in another app. What
does each app's export look like, which of its columns name a printing the way
Scryfall does, and how does it write a finish and where a card is kept? This
is the spec [ADR-0029](../adr/0029-a-collection-import-is-read-by-header-and-a-printing-is-pinned-only-where-scryfall-agrees.md)
and `chip-decklist`'s `collection_import` are built on.

## Method

Gathered 2026-10-08. No app was driven by hand: every app here either needs an
account, a paid tier or a phone. Instead, each claim rests on real export files
committed to public GitHub repos (pinned to a commit), the app's own help
pages, or someone's importer for the format, and is labelled with which.

The reader was then run over six of those real exports, unmodified:

| File | Source | Rows | Copies | Places | Unreadable | Left behind |
|---|---|---|---|---|---|---|
| [manabox2](https://github.com/massimilianobotticelli/magic-strategist/blob/9b5221e40505dc066bcf0c49ebc34c72d5ea5428/data/manabox/2026-08-13/ManaBox_Collection.csv) | ManaBox | 495 (+6 in a list) | 570 | 5 | 0 | Purchase price 487, Condition 495, Language 2 |
| [dragon-shield](https://github.com/Chazmus/dragonshield-converter/blob/23d65e14d81ff89eb2223fbe5610c20923230250/tests/resources/dragon-shield.csv) | Dragon Shield | 2,039 | 2,973 | 3 | 0 | Condition 2,039, Price Bought 501 |
| [moxfield haves](https://github.com/Mambokara/Magic_Binder_Filler/blob/3ae932d2ff3977a9775de9bd0453d0bfa4b63e78/moxfield_haves_2024-08-29-1530Z.csv) | Moxfield | 93 | 142 | 0 | 0 | Condition 93, Language 92 |
| [archidekt sample](https://github.com/john-alexander-611/edhbulkuptests/blob/0c264656d8e88d67dba771d9f55e344604f716a4/assets/archidekt_sample.csv) | Archidekt | 8 | 8 | 0 | 0 | Condition 8 |
| [deckbox inventory](https://github.com/ynvaser/deckscraper/blob/bfce2dd71227ffc373b9c3d7e015d2aaa07171b3/src/test/resources/csv/deckbox_inventory_example.csv) | Deckbox | 5 | 20 | 0 | 0 | Condition 4, My Price 5 |
| [manabox1](https://github.com/nwchinn/mtg-agent/blob/2083a840fc4142b5434c9ca28b7682c5a55d8bfa/data/Ex_ManaBox_Collection.csv) | ManaBox | 2 | 2 | 1 | 0 | Purchase price 2, Condition 2 |

The ManaBox and Dragon Shield files were then imported in Curator against
Scryfall. Every ManaBox row was pinned by its Scryfall ID. Of Dragon Shield's
2,039 rows, all but one were pinned by set and number with Scryfall agreeing
on the name; the one, `PLST 800`, is a number Scryfall does not have, and went
in by name.

## What the importer does with it

| App | Recognised by | Printing named by | Finish | Place |
|---|---|---|---|---|
| ManaBox | `ManaBox ID` | Scryfall ID | `Foil`: normal / foil / etched | `Binder Name`; a `list` is skipped |
| Moxfield | `Tradelist Count` + `Collector Number` | `Edition` + `Collector Number`, checked | `Foil`: "" / foil / etched | — |
| Deckbox | `Tradelist Count` + `Card Number` | Scryfall ID if exported, else name | `Foil`: "" / foil | — |
| Archidekt | `Edition Code` | Scryfall ID | `Finish`: Normal / Foil / Etched | — |
| Dragon Shield | `Folder Name` + `Card Name` | `Set Code` + `Card Number`, checked | `Printing`, every `… Foil` a foil | `Folder Name` |
| Sorted | `List Name` + `Card Name` | `Set Code` + `Card Number`, checked | `Printing` | `List Name` |
| TCGplayer | `Simple Name` | `Set Code` + `Card Number`, checked | `Printing`: Normal / Foil | — |
| Helvault | `scryfall_id` + `extras` | Scryfall ID | `extras`: "" / foil / etchedFoil | — |
| MTGGoldfish | `Set ID` + `Card` | Scryfall ID if filled, else `Set ID` + number, checked | `Foil`: regular / foil / foil_etched | — |
| TopDecked | `SETCODE` + `COLLECTOR NUMBER` | `ID` (Scryfall's) | `FINISH` | — |
| Any other CSV | a name or Scryfall ID column | the commonest column names | the commonest words | `Binder`, `Folder`, `Location` |
| Text | no header | `(SET) number` when given, checked | `*F*`, `*E*` | — |

*Checked* means the printing is pinned only where Scryfall's card at that set
and number has the row's name (a double-faced card's front face counts). The
apps that write codes of their own, Dragon Shield's guild kits, TCGplayer's
`LIST`, MTGGoldfish's MTGO codes, then fall back to the name instead of naming
a different card, and the import lists each one.

Not supported, on purpose:

- **TCGplayer's `Product ID`.** Scryfall's `/cards/collection` takes no
  TCGplayer id, and `/cards/tcgplayer/:id` is one request per card, which at
  Scryfall's rate is minutes for a binder. Set and number, checked, covers most.
- **Delver Lens `.dlens`.** Its card ids resolve only through a database inside
  Delver's app package (section 7). Its CSV presets are read as the app each
  imitates.
- **A file that counts copies in columns this does not read** (Delver's EchoMTG
  preset, `Reg Qty` and `Foil Qty`) is refused, not read as one copy a row.
- **Condition, language, price, tags** are counted per column and named in the
  import, since the collection has no key for them yet (ADR-0023).

The rest of this file is the evidence, app by app.

## Labels
Every claim carries one of these:

- **OBSERVED**: read in a real export file committed to a public repo. The link
  is pinned to the commit that added or last touched the file.
- **DOCS**: the app's own help page, or another site's official format page.
- **PARSER**: inferred from someone's importer or converter code, or from a
  maintainer's written notes about their round-trips.
- **UNVERIFIED**: I could not confirm it. Each says why.

## The main sample source, and its limits

One repo holds most of the evidence: StepKie/MtgCsvHelper at
`e8488c7c982bf4663d62e184cf23e34ce11e7b34`. Abbreviated **MCH** below:

- `MCH/` = `https://github.com/StepKie/MtgCsvHelper/blob/e8488c7c982bf4663d62e184cf23e34ce11e7b34/MtgCsvHelper/Resources/SampleCsvs/`
- `MCH-SB` = `MCH/Tests/SITE_BEHAVIOR.md`, the maintainer's log of live imports
  and re-exports against each site.

How far each kind of MCH file can be trusted:

- The `Tests/*-real-export.csv` files are a fixed reference set. It was imported
  into each site and then re-exported from the site, so those bytes are the
  site's own export output. The files were renamed in commit `556bbc5`
  (2026-06-10); the round-trips are dated May–June 2026.
- **Exceptions:** `tcgplayer-real-export.csv` and `mtggoldfish-real-export.csv`
  are marked *synthetic* in MCH-SB, because both sites gate import behind a
  paid tier. Do not cite them.
- The `Collection/*.csv` files are real user exports, added 2023–2024. Each
  section gives the commit.
- The `Reference/*.csv` files are **generated by MCH's writer**. They are not
  evidence and I ignored them.

### Line endings

Git may normalise line endings. Several MCH files are LF where MCH-SB says the
site needs CRLF. Files committed by other people keep CRLF and are flagged
`[CRLF]` below. **None of the samples had a UTF-8 BOM.**

---

## 1. ManaBox

### Header rows

Three variants are observed. Order is fixed within each; case is exactly as
shown; values are quoted only when needed (the RFC 4180 minimum).

**(a) Whole-collection export**, which starts with binder columns:

```
Binder Name,Binder Type,Name,Set code,Set name,Collector number,Foil,Rarity,Quantity,ManaBox ID,Scryfall ID,Purchase price,Misprint,Altered,Condition,Language,Purchase price currency
```

- OBSERVED, 3,743 rows: `MCH/Collection/manabox-collection.csv` (commit
  d07647e, 2023-12-20).
- OBSERVED: [nwchinn Ex_ManaBox_Collection.csv@2083a84 (2025-05)](https://github.com/nwchinn/mtg-agent/blob/2083a840fc4142b5434c9ca28b7682c5a55d8bfa/data/Ex_ManaBox_Collection.csv).
- OBSERVED `[CRLF]`: [chayde ManaBox_Collection.csv@24877a0 (2026-03)](https://github.com/chayde/mtg-deck-collection/blob/24877a072c5a629a2adbb5864c4cf3b737af1834/ManaBox_Collection.csv).

**(a') The same, with `Added` appended** (mid-2026):

```
Binder Name,Binder Type,Name,Set code,Set name,Collector number,Foil,Rarity,Quantity,ManaBox ID,Scryfall ID,Purchase price,Misprint,Altered,Condition,Language,Purchase price currency,Added
```

- OBSERVED `[CRLF]`: [massimilianobotticelli ManaBox_Collection.csv@9b5221e (2026-08-13)](https://github.com/massimilianobotticelli/magic-strategist/blob/9b5221e40505dc066bcf0c49ebc34c72d5ea5428/data/manabox/2026-08-13/ManaBox_Collection.csv).
- OBSERVED `[CRLF]`: [santiagosan93 ManaBox_Collection.csv@b2d86e6 (2026-08)](https://github.com/santiagosan93/ManaBox-Commander-Deck-Builder-Agent/blob/b2d86e604d9dc1fa88dd33cc26c05c41328b5d52/ManaBox_Collection.csv).
- `Added` is ISO-8601 UTC with milliseconds: `2026-07-11T18:44:33.509Z`.

**(b) Single binder or list export**, with no binder columns:

```
Name,Set code,Set name,Collector number,Foil,Rarity,Quantity,ManaBox ID,Scryfall ID,Purchase price,Misprint,Altered,Condition,Language,Purchase price currency
```

- OBSERVED: `MCH/Tests/manabox-real-export.csv` (round-trip, 2026-05/06).
- OBSERVED `[CRLF]`: [erichschroeter ManaBox.collection.exported.csv@023b306 (2024-07)](https://github.com/erichschroeter/ijw2pmtg/blob/023b30629df675c70cf1ab3380531023ed9f6e04/examples/ManaBox.collection.exported.csv).
- With `,Added` appended, OBSERVED `[CRLF]`: [Neo2854 All.csv@1687c94 (2026-10-07)](https://github.com/Neo2854/mana-binder/blob/1687c9462fc68e01c64a19163ad3d0decfc3ab43/All.csv).

**(c) Newest, October 2026**, which adds `Signed` and `Proxy`:

```
Name,Set code,Set name,Collector number,Foil,Rarity,Quantity,ManaBox ID,Scryfall ID,Purchase price,Misprint,Altered,Signed,Condition,Language,Proxy,Purchase price currency,Added
```

- PARSER/fixture: [eth0net manaweb fixtures@2d19868 (2026-10-06)](https://github.com/eth0net/manaweb/tree/2d1986888e5c7841c0587d778986560d51e1f580/web/src/import/fixtures).
  The commit message says "They added Signed and Proxy to the export … as
  their own export revealed it". `manabox-list.csv` and `manabox-binder.csv`
  look real. The repo's own history says `manabox-collection.csv` is
  synthetic.
- **UNVERIFIED:** whether the whole-collection form now also carries `Signed`
  and `Proxy` after `Binder Name,Binder Type`.

**Takeaway:** ManaBox appends and inserts columns over time. **Match by header
name, never by position.**

### Export behaviour (DOCS)

From [manabox.app/guides/collection/import-export](https://www.manabox.app/guides/collection/import-export/):

- A whole-collection export "will include all card properties as well as the
  binder/list name".
- A single binder or list can be exported from its own screen. That matches
  variant (b), which has no binder column.

### Printing key

- `Scryfall ID` is a Scryfall card UUID. OBSERVED and filled on every row in
  every sample.
- `Set code` is the Scryfall set code in **UPPERCASE**, with Scryfall's own
  codes for specials. OBSERVED: `TMH2`, `TCLB`, `PLST`, `PUMA`, `SLD`, `30A`.
- `Set name` is Scryfall's set name. OBSERVED: `Modern Horizons 2 Tokens`,
  `The List`, `Ultimate Box Topper`.
- `Collector number` is Scryfall's collector number verbatim. OBSERVED: `741z`,
  `U8`, `DDC-49`, `CN2-22`.
- `ManaBox ID` is ManaBox's own internal integer.
- On import, ManaBox needs `Name` plus `Set code` or `Set name`, or a `Scryfall
  ID` alone. Set names and codes "are the same ones from Scryfall". DOCS.

### Finish

- Column `Foil`, values `normal` / `foil` / `etched`. OBSERVED: all three
  appear in `manabox-collection.csv` (3,576 / 166 / 1) and in the round-trip
  file.

### Location

- `Binder Name` is free text. OBSERVED: `Import`, `my list`,
  `FNM 2023-10-20`, `Proxies`.
- `Binder Type` is one of `binder` / `list` / `deck`. OBSERVED: all three in
  `manabox-collection.csv` (3,740 / 2 / 1). The massimilianobotticelli sample
  has `deck`.
- **Quirk:** deck contents appear in the whole-collection export as rows with
  `Binder Type=deck`. An importer that treats every row as an owned card
  double-counts deck copies of cards that are also in binders.
  **UNVERIFIED:** whether ManaBox decks hold their own copies or reference
  collection copies.

### Condition

- OBSERVED, `manabox-collection.csv` plus the round-trip: `mint`, `near_mint`,
  `excellent`, `good`, `light_played`, `played`, `poor`. Seven values.

### Language

- OBSERVED, round-trip: `en`, `es`, `fr`, `de`, `it`, `pt`, `ja`, `ko`, `ru`,
  `zh_CN`, `zh_TW`. Lowercase ISO-ish, except Chinese.
- PARSER (eth0net commit message, above): ManaBox's exporter writes Phyrexian
  cards as English, and its importer refuses Phyrexian. Not verified.

### Other columns

- `Rarity`: `common`, `uncommon`, `rare`, `mythic`, `special`. OBSERVED.
- `Misprint`, `Altered`, and later `Signed`, `Proxy`: `true` / `false`.
- `Purchase price`: a plain decimal with at most one trailing zero (`12.0`,
  `1.5`; PARSER, MCH-SB).
- `Purchase price currency`: `USD`, `EUR` or `GBP`, all OBSERVED.

### Names, tokens, art cards

- Double-faced, adventure, split and MDFC cards all use the full
  `Front // Back`. OBSERVED: `Brazen Borrower // Petty Theft`,
  `Delver of Secrets // Insectile Aberration`, `Jin-Gitaxias // The Great Synthesis`.
- Tokens use plain Scryfall names in Scryfall token sets. OBSERVED: `Clue`,
  `TMH2`, `14`.

### Import quirks (PARSER, MCH-SB)

- Rows with unquoted commas fail.
- Import accepts extra columns, including `Binder Name` and `Binder Type`.
- On import, ManaBox fills in a missing purchase price with the current market
  price.

---

## 2. Moxfield

### Collection export header

All fields are quoted, including numbers; CRLF line endings.

```
"Count","Tradelist Count","Name","Edition","Condition","Language","Foil","Tags","Last Modified","Collector Number","Alter","Proxy","Purchase Price"
```

The header is unchanged from 2024-03 to 2026-09. OBSERVED in:

- [henrilhos moxfield_haves.csv@bf90286 (2024-04)](https://github.com/henrilhos-archives/magic/blob/bf90286cbbc3b82ba226ca2045c8902166f6b8a8/raw-data/moxfield_haves.csv) `[CRLF]`
- [Mambokara moxfield_haves_2024-08-29-1530Z.csv@3ae932d](https://github.com/Mambokara/Magic_Binder_Filler/blob/3ae932d2ff3977a9775de9bd0453d0bfa4b63e78/moxfield_haves_2024-08-29-1530Z.csv) `[CRLF]`
- [hobosock moxfield_haves_2025-01-20-1859Z.csv@d68c5ae](https://github.com/hobosock/decklist/blob/d68c5aed940ab57bd533f74fe02ca816474a09c7/moxfield_haves_2025-01-20-1859Z.csv) `[CRLF]`
- [austingriffith94 moxfield_collection.csv@a501175 (2026-10)](https://github.com/austingriffith94/mtg_collection/blob/a501175a3f0cdeca51c0ff9ccc5e65b1b69452fd/moxfield_samples/moxfield_collection.csv)
- `MCH/Collection/moxfield-haves.csv`: 7,385 rows, timestamps up to 2025-06.
- `MCH/Tests/moxfield-real-export.csv`: round-trip, 2026-05.

The default download filename is `moxfield_haves_<YYYY-MM-DD-HHMM>Z.csv`.
OBSERVED in the filenames above.

### Import header (DOCS)

Source: Moxfield help, "Importing a Collection", last updated 2023-10-22.
moxfield.com itself is Cloudflare-blocked to scripts; I read it through
[web.archive.org snapshot 20260325010706](https://web.archive.org/web/20260325010706/https://moxfield.com/help/importing-collection).

- Expected headers: `Count, Name, Edition, Condition, Language, Foil,
  Collector Number, Alter, Playtest Card, Purchase Price`.
- They "must be spelled exactly as above, including case … the order of the
  column headers is irrelevant".
- Only `Name` is required.
- **Mismatch with the export:** the docs say `Playtest Card`, but every real
  export says `Proxy`. The export also adds `Tradelist Count`, `Tags` and
  `Last Modified`. **Read both `Proxy` and `Playtest Card`.**
- The docs' example rows are fully quoted:
  `"1","Adrix and Nev, Twincasters","oc21","LP","English","etched","9","TRUE","",""`.
- Import has a per-source format picker: Moxfield, Archidekt, CardSphere,
  DeckBox, Decked Builder, DeckStats, Helvault, ManaBox, TappedOut. DOCS, same
  page.
- Collection Location (`Paper` / `Arena` / `MTGO`) and Binder are chosen in
  the import dialog, not in the CSV. DOCS.

### Printing key

- `Edition` is the Scryfall set code in **lowercase**. OBSERVED: `tmh2`,
  `tclb`, `plst`, `puma`, `sld`, `pdgm`, `ppro`, `40k`, `unf`.
- DOCS: "Moxfield uses Scryfall set codes".
- `Collector Number` is Scryfall's verbatim. OBSERVED: `741z`, `U8`, `DDC-49`,
  `152★`, `72b`, `2022-1`. DOCS: "Rarely, a letter or star (★) will be
  appended".
- **No Scryfall ID, multiverse id or TCGplayer id column.**
- Resolution on import is anchored on Name. A blank Name gives
  `No card name found. on line N`. Moxfield fuzzily corrects wrong collector
  numbers within a set. PARSER, MCH-SB.

### Finish

- Column `Foil`, values `""` (empty), `foil`, `etched`.
- OBSERVED: `moxfield-haves.csv` has 6,752 empty, 630 `foil`, 3 `etched`.
- DOCS: "Special treatments, such as gilded or textured foils, are not
  accepted" because those printings have their own collector numbers.

### Condition

- **Export** (OBSERVED, 7,385 rows): `Mint`, `Near Mint`,
  `Good (Lightly Played)`, `Played`, `Heavily Played`, `Damaged`.
  - `Good (Lightly Played)` is the LP grade and `Played` is the MP grade:
    Deckbox's ladder.
- **Import** (DOCS) accepts short or long forms:

  | Short | Long |
  |---|---|
  | `M` | `Mint` |
  | `NM` | `Near Mint` |
  | `LP` | `Lightly Played` |
  | `MP` | `Played` |
  | `HP` | `Heavily Played` |
  | `D` or `DM` | `Damaged` |

  The docs never mention `Good (Lightly Played)`, but the exporter writes it
  and re-import accepts it (round-trip, MCH-SB).

### Language

- **Export** uses English names. OBSERVED: `English`, `German`, `French`,
  `Italian`, `Spanish`, `Portuguese`, `Japanese`, `Korean`, `Russian`,
  `Chinese`, `Traditional Chinese`. Simplified Chinese is written `Chinese`.
- **Import** (DOCS) accepts the long name, a code, or a "printed code":

  | Code | Printed code | Language |
  |---|---|---|
  | en | en | English |
  | es | sp | Spanish |
  | fr | fr | French |
  | de | de | German |
  | it | it | Italian |
  | pt | pt | Portuguese |
  | ja | jp | Japanese |
  | ko | kr | Korean |
  | ru | ru | Russian |
  | zhs | cs | Simplified Chinese |
  | zht | ct | Traditional Chinese |
  | he | | Hebrew |
  | la | | Latin |
  | grc | | Ancient Greek |
  | ar | | Arabic |
  | sa | | Sanskrit |
  | ph | ph | Phyrexian |

### Location

- **No binder or location column in the export.** OBSERVED in every sample.
- Moxfield has binders and "Collection Location" (DOCS), but neither appears
  in the CSV.
- MCH-SB says "binder and collection exports differ" and mentions a
  `Folder Name` column. It does not show such an export, and its own
  historical note attributes `Folder Name` to Dragon Shield's
  Moxfield-format export. **UNVERIFIED; treat as false until a real binder
  export shows up.**

### Other columns

- `Tradelist Count` is independent of `Count`.
- `Alter` and `Proxy` are always written `False` or `True`. Import takes
  `TRUE`/`FALSE`/blank.
- `Last Modified` looks like `2025-01-05 19:26:45.563000`: no timezone,
  microseconds.
- `Purchase Price` has no currency symbol and may be blank.
- `Tags` was empty in every sample. **UNVERIFIED:** how multiple tags are
  written.

### Names and tokens

- Double-faced, split and adventure cards use the full `A // B`. OBSERVED:
  `Brazen Borrower // Petty Theft`, `Ambitious Farmhand // Seasoned Cathar`.
- Tokens use the plain Scryfall name and token-set code (`Clue`, `tmh2`, `14`).
  `Arlinn, Embraced by the Moon Emblem` / `tinr` is OBSERVED.
- Moxfield rejects `Clue Token`. PARSER, MCH-SB.
- Unfinity blanks (`_____ Goblin`) are OBSERVED as-is.

---

## 3. Archidekt

### Export header

```
Quantity,Name,Finish,Condition,Date Added,Language,Purchase Price,Tags,Edition Name,Edition Code,Multiverse Id,Scryfall ID,Collector Number
```

- OBSERVED: `MCH/Tests/archidekt-real-export.csv` (round-trip, 2026-05).
- OBSERVED: [john-alexander-611 archidekt_sample.csv@0c26465 (2026-09)](https://github.com/john-alexander-611/edhbulkuptests/blob/0c264656d8e88d67dba771d9f55e344604f716a4/assets/archidekt_sample.csv).
- Values are quoted only when needed.
- **The export's columns are user-selectable.** The header above is the
  default selection. Sources:
  - Archidekt staff on the [forum thread "Export Collection Column Options" (2025-03)](https://archidekt.com/forum/thread/11781256?page=1)
    discuss which columns are "available … when exporting your entire
    collection".
  - The [MythicHub help](https://mythichub.com/help/importing-collection) says
    to keep "Scryfall ID" selected.
  - Another fixture has an extra `MTGO ID` column before `Collector Number`:
    [Leyline-Coding/scryme archidekt_sample.csv@766d696](https://github.com/Leyline-Coding/scryme/blob/766d696731d67c8626bd542850d69d6d2fb2c01b/backend/tests/fixtures/archidekt_sample.csv).
    It is synthetic (fake UUIDs) but hints at an optional column.
- **UNVERIFIED:** the full list of selectable export columns.

### Printing key

- `Scryfall ID`: UUID. OBSERVED, every row.
- `Edition Code`: Scryfall code in **lowercase** (`plst`, `puma`, `tclb`,
  `h2r`). OBSERVED.
- `Edition Name`: Scryfall's set name. OBSERVED: `Ultimate Box Topper`,
  `Secret Lair Drop`, `Battle for Baldur's Gate Tokens`.
- `Collector Number`: Scryfall's (`741z`, `DDC-49`, `U8`). OBSERVED.
- `Multiverse Id`: **`0` when the printing has none**, never blank. OBSERVED
  for foil-only printings, tokens, `plst` and `sld`.

### Finish

- Column `Finish`, values `Normal` / `Foil` / `Etched`. OBSERVED.

### Condition

- OBSERVED: `NM`, `MP`, `HP`, `D`.
- `LP` is a valid UI value (PARSER, MCH-SB), so the ladder is
  `NM, LP, MP, HP, D`.
- **UNVERIFIED:** whether there is a Mint, Excellent or graded value.

### Language

- OBSERVED: `EN`, `DE`, `FR`, `IT`, `PT`, `RU`, `JP`, `KR`, `CS`, `CT`.
- These are not ISO: `JP`, `KR`, `CS` = Simplified, `CT` = Traditional.
- `ES` is presumably Spanish, but in MCH's round-trip Spanish became `EN`
  through Archidekt's Moxfield cross-importer.

### Location

- **No binder or location column observed.** Archidekt's collection has
  `Tags`; it was empty in all samples, so the tag separator is
  **UNVERIFIED**.

### Other columns

- `Date Added` is `YYYY-MM-DD`. OBSERVED.

### Names and tokens

- Full `A // B` for transform, adventure, split and MDFC cards. OBSERVED.
- Tokens use plain names (`Clue`, `Treasure`).

### Import (DOCS)

Source: [Archidekt dev update "Collection Importer", 2023-11-15](https://archidekt.com/news/5891613).

- Columns are set positionally, by hand or from a preset per source site.
  "Scryfall ID alone removes ambiguity, or card name with set name/set code and
  collector number."
- Ambiguous rows can be skipped or tagged "Unsure".
- The new importer always adds rows as new entries rather than merging.

Other import notes:

- Presets offered: Cardsphere, Deckbox, Delver Lens, Dragonshield, Helvault,
  ManaBox, Moxfield. There is **no Archidekt-format preset**. PARSER, MCH-SB
  (June 2026).
- Archidekt staff [confirm the Dragon Shield export is "configurable … depending on the user"](https://archidekt.com/forum/thread/6162413), 2023-12.
- Leading zeros in collector numbers (`033`) broke matching. [forum, 2023-05](https://archidekt.com/forum/thread/4533359/1).

---

## 4. Deckbox

### Header

The leading columns are fixed and the trailing ones are user-selectable.

**Minimal and old form** (2017–2023):

```
Count,Tradelist Count,Name,Edition,Card Number,Condition,Language,Foil,Signed,Artist Proof,Altered Art,Misprint,Promo,Textless,My Price
```

- OBSERVED: [jjallaire rna-deckbox.csv@de4e887 (2019-02)](https://github.com/jjallaire/draftpod/blob/de4e8877a3a7b6717526ad3b7d695450078cba43/tests/unit/data/upload/rna-deckbox.csv).
- OBSERVED: [gozer Deckbox-specials.csv@2888d52 (2020-01)](https://github.com/gozer/mtg_processing/blob/2888d52359a7e5ac5180a4dc739ec60fc4b90441/Deckbox-specials.csv).
- OBSERVED: [ynvaser deckbox_inventory_example.csv@bfce2dd (2023-02)](https://github.com/ynvaser/deckscraper/blob/bfce2dd71227ffc373b9c3d7e015d2aaa07171b3/src/test/resources/csv/deckbox_inventory_example.csv).

**Current form**, with `Edition Code`, `Printing Id`, `Printing Note` and
`Tags`; the price and id columns at the end are optional:

```
Count,Tradelist Count,Name,Edition,Edition Code,Card Number,Condition,Language,Foil,Signed,Artist Proof,Altered Art,Misprint,Promo,Textless,Printing Id,Printing Note,Tags,My Price
```

Observed tails after `My Price`:

- `,Price`: [jlfwilliams Inventory_Finn091_2023.October.07.csv@5d49090](https://github.com/jlfwilliams/mtg-log/blob/5d49090035d06cadd528ce03dd58a429e91039e6/data/Inventory_Finn091_2023.October.07.csv), 4,182 rows.
- `,Scryfall ID`: [levofski Inventory_example.csv@0722ab6 (2025-06)](https://github.com/levofski/mtg-collection-analyser/blob/0722ab66ee966f6b65fc522cce52c5f0379554f6/docs/Inventory_example.csv).
- `,Cost,Rarity,Price,TcgPlayer ID,Scryfall ID`: `MCH/Tests/deckbox-real-export.csv` (2026-05).
- Extra `Decks Count Built,Decks Count All` after `Tradelist Count`, plus
  `Type,Cost,Rarity,Price,Image URL,Last Updated,TcgPlayer ID,Scryfall ID` at
  the end: [PsykoFant Inventory_subset.csv@c4144f0 (2024-11)](https://github.com/PsykoFant/CollectaMundo/blob/c4144f0ec3cd81b5bc2a2f1fea943febaf8b9b96/CollectaMundo/TestFiles/Inventory_subset.csv).

**Match by header name.** The default filename is
`Inventory_<user>_<YYYY>.<Month>.<DD>.csv` (OBSERVED, jlfwilliams).

### Import (forum and DOCS)

- Minimum columns are "Count and Name". The error text is "The header of the
  csv file is not correct. Please check that the columns in your file contain
  at the minimum Count and Name". [deckbox forum 30026](https://deckbox.org/forum/topics/30026?p=1).
- A staffer or power user lists import headers "Specifically in that order":
  `Count, Tradelist Count, Name, Edition, Condition (Mint, Near Mint, Good (Lightly Played), Played, Heavily Played, Damaged), Language, Foil (foil or blank), Signed, Artist Proof, Misprint, Promo, Textless, My Price`.
  [deckbox forum 29071 (2017)](https://deckbox.org/forum/topics/29071).
- Deckbox's own help says to use UTF-8. [deckbox.org/help/exports_and_imports](https://deckbox.org/help/exports_and_imports), DOCS.
  - A 2018 forum user found **only ANSI worked and UTF-8 failed** on import
    ([forum 30026](https://deckbox.org/forum/topics/30026?p=1)).
  - Conflicting; **UNVERIFIED** today. For our importer, read UTF-8 first and
    fall back to Windows-1252.
- MTGGoldfish documents the Deckbox header as
  `Count,Tradelist Count,Name,Edition,Card Number,Condition,Language,Foil` and
  says "Edition is the name of the set".
  [mtggoldfish.com/help/import_formats](https://www.mtggoldfish.com/help/import_formats), DOCS.

### Printing key: Deckbox's own vocabulary, not Scryfall's

- `Edition` is the **Deckbox edition name**. It diverges from Scryfall
  (OBSERVED, `MCH/Tests/deckbox-real-export.csv`):

  | Scryfall | Deckbox |
  |---|---|
  | `Secret Lair Drop` | `Secret Lair Drop Series` |
  | `Ultimate Box Topper` | `Ultimate Masters: Box Toppers` |
  | `Modern Horizons 2 Tokens` | `Extras: Modern Horizons 2` |
  | `<Set> Art Series` | `Extras: <Set> Art Series` |

  - Prerelease promos go to `Prerelease Events` or `Prerelease Events: <Set>`.
  - Clash pack promos go to `Magic Origins Clash Pack Promos`.
  - Sources: OBSERVED in PsykoFant and jlfwilliams; [forum 31655](https://deckbox.org/forum/topics/31655)
    for `Extras:` and `Emblem: X` naming.
- `Edition Code` is **Deckbox's own code**, not Scryfall's:
  - Old sets use the Gatherer/MTGO 2-letter codes: `1e` (Alpha), `ap`, `al`,
    `3E`, `4E`, `5E`, `6E`, `7E`, `AN`, `CG` (Urza's Destiny), `GU` (Urza's
    Legacy), `UZ`, `TE`, `MI`, `VI`, `WL`, `IN`, `PS`, `NE`, `PR`, `OD`, `P2`,
    `P3`, `PO`, `BR`, `UG`.
  - Internal codes: `ex_127`, `ex_145`, `ex_137` for token and art-series
    sets; `ptc`, `ptc_10`; `cp3`; `plist` (Scryfall: `plst`).
  - Modern sets mostly match Scryfall (`ltr`, `snc`, `sld`, `puma`, `30a`,
    `uma`, `cmm`, `sta`, `khm`).
  - **Case changed:** UPPERCASE in Oct 2023 (`EMN`, `5E`, jlfwilliams) and
    lowercase in 2025/2026 (`inr`, `ltr`, `1e`, levofski and MCH).
  - OBSERVED for all of the above.
- `Card Number` is **reshaped from Scryfall** (OBSERVED, MCH real export
  against Archidekt's export of the same cards):

  | Card | Scryfall | Deckbox |
  |---|---|---|
  | Aragorn LTR | `741z` | `741` |
  | Alpha Demonic Tutor | `104` | `13` |
  | Lim-Dûl's Vault | `107` | `192` |
  | PUMA Demonic Tutor | `U8` | `8` |
  | The List | `DDC-49` | `49`, with `Printing Note`=`DDC` |
  | Secret Lair Viscera Seer (PARSER, MCH-SB) | `VS` | `801` |

  Old sets use Deckbox's own numbering. A forum note says TCGplayer's `55a`/`55b`
  became Deckbox `17`/`18` ([forum 30026](https://deckbox.org/forum/topics/30026?p=1)).
- `Printing Id` is Deckbox's internal integer.
- `TcgPlayer ID` is a TCGplayer product id; it may be blank.
- **`Scryfall ID`, where selected, is the reliable key.** OBSERVED as correct
  even where `Card Number` was reshaped: `plist`/`49` →
  `77be13ed-…` = Scryfall `plst DDC-49`.

### Finish

- Column `Foil`, values `""` or `foil`. OBSERVED.
- **No etched value:** Deckbox collapses etched to `foil`, and its native
  import rejects `etched`. PARSER, MCH-SB.
- The flags `Signed`, `Artist Proof`, `Altered Art`, `Misprint`, `Promo` and
  `Textless` hold the lowercase flag word when set (`signed`, `proof`) and are
  empty otherwise. OBSERVED, gozer.

### Condition

- OBSERVED: `Mint`, `Near Mint`, `Good (Lightly Played)`, `Heavily Played`,
  and **blank** (most rows in jlfwilliams: 4,173 of 4,182).
- `Played` and `Damaged` are documented on forum 29071.
- **Expect blank condition.**

### Language

- Full English names. OBSERVED: `English`, `Spanish`, `Portuguese`, `Korean`,
  `French`, `German`, `Russian`, `Traditional Chinese`, `Chinese` (=
  Simplified), `Italian`, `Japanese`.
- Can be blank. OBSERVED, ynvaser.

### Location

- **No location column.** Deckbox has one inventory, plus the `Tradelist Count`
  and `Tags` columns. `Decks Count Built` and `Decks Count All` are derived
  counts, not locations.

### Prices and other values

- Prices carry a leading `$` (`$0.00`, `$5.48`).
- `Rarity` uses Deckbox's own words: `MythicRare`, `BasicLand`, `Common`,
  `Special`.

### Names

- Full `A // B` for transform, adventure, split and MDFC. OBSERVED:
  `Delver of Secrets // Insectile Aberration`, `Chosen of Markov // Markov's Servant`.
- Art cards appear as `Art Card: Edgar, Charmed Groom` in `Extras: … Art Series`.
  OBSERVED, PsykoFant.
- Diacritics are kept (`Lim-Dûl's Vault`). OBSERVED.

---

## 5. Dragon Shield MTG Card Manager, and its successor "Sorted"

### Web export header (mtg.dragonshield.com)

The **first line is the literal `"sep=,"`, with the quotes**, then a
15-column header. The last three are price columns, and they **vary**.

```
"sep=,"
Folder Name,Quantity,Trade Quantity,Card Name,Set Code,Set Name,Card Number,Condition,Printing,Language,Price Bought,Date Bought,LOW,MID,MARKET
```

OBSERVED in:

- [Chazmus dragon-shield.csv@23d65e1 (2023-01)](https://github.com/Chazmus/dragonshield-converter/blob/23d65e14d81ff89eb2223fbe5610c20923230250/tests/resources/dragon-shield.csv) `[CRLF]`, 2,039 rows.
- [laukcode my_cards.csv@6a3e417 (2024-04)](https://github.com/laukcode/Mtg_Collection/blob/6a3e417583f8c6add800785ec5b229150bff68ca/DataInput/my_cards.csv).
- `MCH/Tests/dragonshield-real-export.csv` (2026-05).
- An Archidekt forum post (2023-12) quotes the same.

The price-column variant `...,Date Bought,AVG,LOW,TREND`:

- OBSERVED: `MCH/Collection/dragonshield-collection.csv` (commit 57cf93d,
  2023-12-13; German cards, euro-ish prices) and
  `MCH/Tests/dragonshield-tokens-real-export.csv`.
- AVG/LOW/TREND are Cardmarket's price names and LOW/MID/MARKET are
  TCGplayer's, so the variant probably follows the user's price-source setting.
  **This is inference; UNVERIFIED.**

### Other Dragon Shield shapes

- **12-column, no `sep=` line, no price columns:** `MCH/Collection/dragonshield-sample-official.csv`
  (commit c70c3a1, 2026-05). MCH-SB calls it Dragon Shield's published "format
  guide" sample, "a template, not the real export shape". Dates there are
  `M/d/yyyy` (`3/28/2021`).
  - A 12-column CRLF file with `dd/MM/yyyy` dates also exists:
    [reinonlein dubbel.csv@dab1c41](https://github.com/reinonlein/magic-streamlit/blob/dab1c410622d0a473cd227f79f5852cd41a0bbcf/data/dubbel.csv).
    Possibly re-saved through Excel in a Dutch locale. **Provenance unclear.**
- **Mobile app export** is a different header. PARSER, quoted by the MCH
  author from his own app export in
  [StepKie/MtgCsvHelper issue #7 (2024-04)](https://github.com/StepKie/MtgCsvHelper/issues/7):

  ```
  Quantity, Name, CardNumber, Expansion Code, Expansion Name, PurchasePrice, Foil, Condition, Language, PurchaseDate, Single Current Price, Total Current Price
  ```

  - `Foil` is `true`/`false`.
  - The set code is lowercase (`mid`).
  - `PurchaseDate` is `20220129`.
  - **Each face of a double-faced card is written as its own row**
    (`Ambitious Farmhand` and `Seasoned Cathar`, each qty 1).
  - The header has a space after each comma.
  - Not seen in any committed file. **UNVERIFIED beyond that issue.**
- A trailing junk row with an empty `Quantity` appears at the end of the web
  export. PARSER, [KarmaKamikaze convert.py@93ce734](https://github.com/KarmaKamikaze/DragonShield-to-Moxfield/blob/93ce73447e5b5381f93041b9ba6fe564fbf464d4/convert.py),
  which says "Dragon Shield adds a junk data row at the end". It is not present
  in the Chazmus or laukcode samples, so it may be historical.
- Dragon Shield's native importer needs the exact export shape: `"sep=,"`, all
  15 columns, CRLF. It rejects anything else with a generic "please check
  structure" message. PARSER, MCH-SB.

### Sorted, the successor (October 2026)

MCH-SB says Sorted replaces the Dragon Shield scanner apps and migrates the
collection on login. PARSER, from the maintainer's log. Sorted's export
(OBSERVED, `MCH/Tests/sorted-real-export.csv`, 2026-10) has a `"sep=,"` first
line and then:

```
List Type,List Name,Collection,Format,Board,Quantity,Card Name,Set Code,Set Name,Card Number,Condition,Printing,Rarity,Language,Price Bought,Date Bought,Parent List Type,Parent List Name,Current Price (tcgplayer_marketsellprice),List Cover Image,Parent List Cover Image
```

- `List Type` is `Folder` or `Deck`.
- `Board` is `Main Deck` or `Sideboard`.
- `Language` uses 2-letter codes, with `jp` for Japanese, `cn` for Simplified
  Chinese and `tw` for Traditional Chinese. PARSER, MCH appsettings plus
  MCH-SB.
- Card and condition columns match Dragon Shield's. Of the condition values,
  `Poor`, `Played`, `LightPlayed` and `Good` are OBSERVED.

### Dragon Shield: location

- `Folder Name` is free text. OBSERVED: `Box 1`, `Spaceman Binder`, `Rares`,
  `Other`, `CP - Painbow`.
- Each export row belongs to one folder.

### Dragon Shield: printing key

**There is no Scryfall ID.**

- `Set Code` is UPPERCASE and **mostly** Scryfall's (`MH2`, `TMH2`, `TDMU`,
  `PCLB`, `MB1`, `G18`). Exceptions:
  - **Guild kits:** `GK2_AZORIU`, `GK1_DIMIR`, set name `Guild Kit: Azorius`.
    Scryfall has `gk2`/`gk1`. PARSER, MCH-SB plus MCH `DragonShieldCodeReadConverter`.
  - **The List:** the original printing is substituted. Scryfall `PLST DDC-49`
    became `DVD #49`. PARSER, MCH-SB.
  - **Gift pack:** `G18` with card number `GP2`, OBSERVED (Chazmus). Scryfall's
    `g18` numbers it `2`. **UNVERIFIED**; check against Scryfall.
  - **Regional codes:** `LEGI` (Legends Italian). PARSER, MCH-SB.
- `Set Name` is what Dragon Shield's own importer resolves by. It ignores the
  code. PARSER, MCH-SB.
- `Card Number` is mostly Scryfall's (`741z`, `109s`, `1537` for MB1).
  OBSERVED.

### Dragon Shield: finish

Column `Printing`. It is open-ended:

- OBSERVED: `Normal`, `Foil`, `Rainbow Foil`, `Double Rainbow Foil`,
  `Gilded Foil`, and **empty**. Empty showed up on 28 promo rows in Chazmus:
  PCLB `109s`, G18.
- MCH-SB adds `Etched`, `Surge Foil` and `Step and Compleat Foil` as observed
  by the maintainer.
- Treat "contains `foil`" as foil, `Etched` as etched, and empty or `Normal`
  as nonfoil.

### Dragon Shield: condition

- OBSERVED: `Mint`, `NearMint`, `Excellent`, `Good`, `LightPlayed`, `Played`,
  `Poor`. CamelCase with no spaces.
- `HeavilyPlayed` and `Damaged` appear only in a converter's mapping table:
  [PhilippeDupont/DragonShieldToMoxfield](https://github.com/PhilippeDupont/DragonShieldToMoxfield),
  PARSER, unpinned. **UNVERIFIED.**

### Dragon Shield: language

- Full English names. OBSERVED: `English`, `German`, `French`, `Italian`,
  `Spanish`, `Portuguese`, `Japanese`, `Korean`, `Russian`,
  `Traditional Chinese`.
- Simplified Chinese is `Simplified Chinese`. PARSER, MCH-SB: a bare `Chinese`
  silently imports as English.

### Dragon Shield: names

- Transform and MDFC cards get the **front face only**: `Delver of Secrets`,
  `Ambitious Farmhand`, `Valki, God of Lies`.
- Adventure and split cards get the **full name**: `Brazen Borrower // Petty Theft`,
  `Consign // Oblivion`, `Fire // Ice`.
- Sources: OBSERVED in `MCH/Collection/dragonshield-collection.csv` and
  laukcode; MCH-SB.
- Some transform cards are written in full, e.g.
  `Bala Ged Recovery // Bala Ged Sanctuary` in the official sample and
  `Blessed Hippogriff // Tyr's Blessing`. **Do not rely on the rule**; resolve
  either form.
- **Tokens are decorated:** `Clue Token`, `Beast Token (4/4)`, `Bird Token`,
  `Copy Token`, `Morph Creature`, `Betrayer of Flesh Emblem`. OBSERVED. Set
  codes are Scryfall's `T…`.

### Dragon Shield: quoting and dates

- Quoting is selective. Names with apostrophes are quoted even without a comma
  (`"Zur's Weirding"`). OBSERVED.
- Dates are `yyyy-MM-dd` in the web export. OBSERVED.

---

## 6. TCGplayer app (and the TCGplayer collection export)

### Header

```
Quantity,Name,Simple Name,Set,Card Number,Set Code,Printing,Condition,Language,Rarity,Product ID,SKU
```

- DOCS: MTGGoldfish publishes exactly this header as "TCGplayer App"
  ([import_formats](https://www.mtggoldfish.com/help/import_formats)).
- OBSERVED: `MCH/Collection/tcgplayer-collection.csv` (1,010 rows, commit
  0e189ae, 2024-09) and `tcgplayer-collection-old.csv` (1,973 rows).
- Confirmed by a [deckbox forum user](https://deckbox.org/forum/topics/30026?p=1)
  (2018) as "the original default headers".
- **Columns are configurable** in the app ("CSV Output Settings"):
  - ManaBox's docs tell users to set "Default share format" to CSV and enable
    "TCGplayer ID" and "Product ID". DOCS, [manabox import-export guide](https://www.manabox.app/guides/collection/import-export/).
  - OBSERVED variants, both in [EchoMTG/mtg-csv-reader@fa0e05e](https://github.com/EchoMTG/mtg-csv-reader/tree/fa0e05e337f5b290b8d46c48e3d98526623113f2/example_datasets/tcgplayer):
    - All options on:
      `Quantity,Name,Simple Name,Set,Card Number,Set Code,External ID,Printing,Condition,Language,Rarity,Product ID,SKU,Price,Price Each`
      (`TCG-Scanner-with-all-options.csv`). `External ID` was `0`; prices
      look like `$182.83`.
    - Names off:
      `Quantity,Card Number,Set Code,External ID,Printing,Condition,Language,Rarity,Product ID,SKU,Price,Price Each`.
    - Minimal: `Quantity,Name,Set Code,Printing,Language`.
  - A 2018 user exported **with no header row at all**
    ([deckbox forum 30026](https://deckbox.org/forum/topics/30026?p=1)). That
    was user-removed in that case, but guard for it.

### Printing key

- `Product ID` is TCGplayer's product id. Scryfall exposes it as
  `tcgplayer_id`, so `/cards/tcgplayer/{id}` resolves it.
- `SKU` is TCGplayer's SKU: product × condition × printing × language.
  OBSERVED: present and numeric in the real files.
- `Set Code` is **TCGplayer's code**, usually Scryfall's in uppercase, but
  OBSERVED exceptions include:

  | TCGplayer | Scryfall / meaning |
  |---|---|
  | `LIST` | `plst` |
  | `PPBLB`, `PPLCI`, `PPWOE`, `PPMOM`, `PPONE`, `PPDMU` | Promo Pack; Scryfall `p<set>` with `p`-suffixed numbers |
  | `ARENA` | Arena Promos |
  | `CMB1` | `cmb1` |
  | `MB1` | |
  | `FNM` | |
  | `EXP` | Zendikar Expeditions |

- `Set` is **TCGplayer's set name**. OBSERVED divergences:
  - `Magic 2015 (M15)`
  - `The List Reprints`
  - `Promo Pack: Bloomburrow`
  - `Universes Beyond: The Lord of the Rings: Tales of Middle-earth`
  - `Mystery Booster Cards`
  - `Outlaws of Thunder Junction: Breaking News`
  - `Mystery Booster: Convention Edition Exclusives`
- `Card Number`: mostly Scryfall's. The List uses `C16-177` / `M19-4`, which
  matches Scryfall's plst form. **Older sets may use TCGplayer's own numbering**
  ([deckbox forum](https://deckbox.org/forum/topics/30026?p=1): "TCGPlayer
  uses different card numbers for older sets", `55a`/`55b`). The old file has
  `Abbey Matron [Version 2]` with number `2b`.

### Finish

- Column `Printing`, values `Normal` / `Foil`. OBSERVED, nothing else in
  ~3,000 rows.
- **UNVERIFIED:** how etched is written. Probably `Foil`, with etched as a
  separate product id.

### Condition

- OBSERVED: `Near Mint`, `Moderately Played`. TCGplayer's ladder is
  `Near Mint, Lightly Played, Moderately Played, Heavily Played, Damaged`.
- The other three are **UNVERIFIED** in a real file but are TCGplayer's
  standard ladder.

### Language

- Full English names. OBSERVED: `English`, `French`, `German`, `Italian`.

### Location

- None.

### Names

- `Name` carries TCGplayer variant decorations, OBSERVED:
  `Accursed Marauder (Retro Frame)`, `(Showcase)`, `(Borderless)`,
  `(Extended Art)`, `Cavern of Souls (0269)`,
  `Abbey Matron [Version 2]` (old file).
- `Simple Name` strips those decorations.
- **Split cards:** `Name` is `Life // Death` but `Simple Name` is `Life`, the
  first half only. OBSERVED.
- Diacritics are folded in both columns (`Lim-Dul`). PARSER, MCH-SB.
- The only `//` names observed were split cards, so transform and MDFC naming
  is **UNVERIFIED**.

### Rarity

- `Common`, `Uncommon`, `Rare`, `Mythic`, `Special`, `Land`, `Promo`.
  OBSERVED.

---

## 7. Delver Lens

Summarised from a research sub-agent; the pins are its own. I re-checked the
EchoMTG samples.

### `.dlens` backup

- It is a SQLite file. Its `cards` table has `_id`, `card`, `foil` (0 or
  non-zero), `quantity`, `language` (text, blank = EN), `condition` (text),
  `creation` (epoch ms), `list` (FK → `lists._id`, which has `name`) and
  `image` (JPEG blob).
- PARSER: [od3zza/delvertocsv script.js@e0db007](https://github.com/od3zza/delvertocsv/blob/e0db007d8871c80b072bb62e9de770d36e4213d4/script.js),
  [shagu/delverexport delver.lua@946709d](https://github.com/shagu/delverexport/blob/946709d5b75c8df29b7ac106bb5181573c476dd9/delver.lua).
- **`cards.card` is Delver's internal id.** Mapping it to `scryfall_id` needs
  the `res/raw/data.db` that ships inside the APK.
  - Older backups embedded `data_cards`, `data_names` and `data_editions`.
  - Newer backups don't: "no such table: data_cards",
    [delvertocsv issue #1](https://github.com/od3zza/delvertocsv/issues/1).
  - PARSER: [Jertzukka/dlensExporter main.py@e454031](https://github.com/Jertzukka/dlensExporter/blob/e454031be293bef3451d8e1e2cf85505347cdf62/main.py),
    [multimeric/DelverLensExtract App.tsx@81a6930](https://github.com/multimeric/DelverLensExtract/blob/81a69303c9a2db87a6115b253c2d65593776782a/src/App.tsx).
  - **Conclusion: a `.dlens` file cannot be imported without Delver's APK
    database.** Archidekt declined it for this reason
    ([forum](https://archidekt.com/forum/thread/7585436/1)). Don't support it.
- Conditions in the backup include `Moderately Played` and the misspelled
  `Slighty Played`. PARSER, dlensExporter.

### CSV export

The CSV export is **user-configured**: column picker, separator choice, and
per-site presets. DOCS: [delverlab.com changelog](https://www.delverlab.com/#changelog).

- Presets: Moxfield (deck and collection), TCGplayer, TCGplayer Direct,
  Archidekt, TopDecked, Card Conduit, UrzaGatherer, CubeCobra, BinderPOS,
  EchoMTG, MTGGoldfish, LigaMagic, Deckbox.
- Optional columns: Scryfall ID, MultiverseID, TCGplayer productID and SKU,
  MKM product ID, MTGO set code, uppercase edition codes, and others.

OBSERVED in [EchoMTG/mtg-csv-reader example_datasets/delver@fa0e05e](https://github.com/EchoMTG/mtg-csv-reader/tree/fa0e05e337f5b290b8d46c48e3d98526623113f2/example_datasets/delver):

- Custom columns:

  ```
  Name,Acquired Price,Language,Set Code,TCGPlayer Product ID,Number,Foil,Quantity
  ```

  - Every field is quoted.
  - `Foil` is `"Foil"` or `""`.
  - Language is `""` for English.
  - Price looks like `"$13.94"`.
- EchoMTG preset:

  ```
  Reg Qty,Foil Qty,Name,Set,Acquired,Language
  ```

  - Set is the full name.
  - Price is locale-formatted: `"€ 1,77"`, `$0.79 ` with a trailing space.
  - Also exported as TSV.
- **Practical upshot:** a "Delver Lens CSV" is whatever preset the user
  picked. Detect the *target-site* preset (Moxfield, Deckbox, …) instead of a
  Delver format.
- Etched may be encoded as an `e` suffix on the collector number (`SLD 708e`).
  User report, [moxfield.nolt.io/1009](https://moxfield.nolt.io/1009).
  **UNVERIFIED.**

---

## 8. MTGGoldfish

### Documented header (DOCS)

Source: [mtggoldfish.com/help/import_formats](https://www.mtggoldfish.com/help/import_formats).

- Header: `Card,Set ID,Set Name,Quantity,Foil,Variation`.
- Foil is `FOIL` / `REGULAR` / `FOIL_ETCHED`.
- **Set codes are Magic Online codes**: "MTGGoldfish uses the Magic Online set
  codes which are sometimes different". The page has the full table. Examples:
  - Old sets: `1E` (Alpha MTGO), `7E`, `AP`, `IN`, `MI`, `UZ`, `UL`, `UD`.
  - Specials: `DD3_DVD`, `MED-GRN`, `PRM-FNM`, `PRM-UMA` (Ultimate Box Topper),
    `PLIST`, `UPLIST`, `MTGA`, `BOOSTER`, `SEALED`.
- `Variation` values: `showcase`, `extended`, `borderless`, `japanese`,
  `planeswalker stamp`, `precon`, `prerelease`, `pw_deck`, `brawl_deck`,
  `buy-a-box`, `promo pack`, `bundle`, `sealed`, `timeshifted`.

### Real export (OBSERVED)

`MCH/Collection/mtggoldfish-collection.csv`, 535 rows, commit 31e7a55,
2024-01-29:

```
Card,Set ID,Set Name,Quantity,Foil,Variation,Collector Number,Scryfall ID
```

- **Foil is lowercase** in the real export: `regular`, `foil`.
- `Variation` held a collector number (`243`) for guildgates, which contradicts
  the docs' vocabulary.
- `Scryfall ID` was **blank on 495 of 535 rows**.
- `Variation` is always quoted (`""`).

The Arena-collection export is
`Card,Set ID,Set Name,Quantity,Foil`, with `Set Name` empty and `Foil` empty.
OBSERVED: `MCH/Collection/mtggoldfish-from-mtgarena.csv` (commit 294496f,
2024-01-31, 6,160 rows).

### Names

- Split cards use the full `Integrity // Intervention`. OBSERVED.
- Double-faced cards appear to be front face only. That rests on the synthetic
  MCH file, so it is **UNVERIFIED** in a real export.

### Other

- Location, condition and language: none.
- MTGGoldfish CSV import is Premium-only. PARSER, MCH-SB.

---

## 9. EchoMTG

All from the sub-agent; not re-checked by me except the repo listing.

- Export header: **UNVERIFIED.** The [Echo FAQ](https://www.echomtg.com/help/faqs/)
  says columns vary by subscription tier.
- The [Echo blog #124](https://www.echomtg.com/blog/124/scryfall-ids-are-live-on-echomtg-import-from-moxfield-archidekt-and-manabox-in-one-click/)
  says the export now adds `Scryfall Id` and `Scryfall Oracle Id`. DOCS.
- API vocabularies (DOCS, [echomtg.com/api](https://www.echomtg.com/api/)):
  - Foil is `1` or `0`.
  - Condition: `NM LP MP HP D`, plus `ALT ART PRE TS SGN` and graded codes
    (`BGS B10…`, `PSA P10…`, `CGC C10…`).
  - Language: `EN DE FR RU IT ES PT CT CS JP KR`.
- Echo's own importer (PARSER, [header_helper.ts@fa0e05e](https://github.com/EchoMTG/mtg-csv-reader/blob/fa0e05e337f5b290b8d46c48e3d98526623113f2/src/helpers/header_helper.ts))
  matches headers case-insensitively against alias lists. This is a useful
  model for our own alias table:
  - Quantity: `reg qty|quantity|count`; foil quantity: `foil qty`.
  - Name: `card name|card|name|title`.
  - Set name: `set|set name|expansion|edition`.
  - Set code: `set_code|code|set code|expansion code|edition code`.
  - TCGplayer id: `product id|tcgid|tcgplayer id`.

---

## 10. Helvault

OBSERVED in fixtures at [charlesrocket/frightcrawler spec/data@a4a221f](https://github.com/charlesrocket/frightcrawler/tree/a4a221f4e17c6f485260995f2834219d1aaabf52/spec/data),
committed 2021-10. Every field is quoted, and **the columns are in
alphabetical order**.

- Free tier: `extras,language,name,quantity,scryfall_id`
- Pro tier: `collector_number,estimated_price,extras,language,name,oracle_id,quantity,rarity,scryfall_id,set_code,set_name`

Values:

- `extras` holds the finish: `""`, `foil` or `etchedFoil`.
- `language` is `en`.
- `set_code` is lowercase Scryfall.
- The printing key is `scryfall_id`, which is always present.
- No condition and no location. DOCS: Moxfield's help says "Helvault doesn't
  currently track card condition".
- **UNVERIFIED:** other `extras` values, such as signed or altered tokens.

---

## 11. Plain-text collection and deck lines

- **ManaBox** (DOCS, [decks import-export guide](https://www.manabox.app/guides/decks/import-export/)):
  the "MTG Arena" style, `3 Verdant Catacombs (MH2) 260`. "The set and number
  are optional." Its collection text import "works the same as in decks". The
  guide does not mention foil markers.
- **Moxfield** (sub-agent; third-party docs and parsers, nothing from Moxfield
  itself reachable):
  - Line: `1 Prosper, Tome-Bound (AFC) 2 *F*`, with `*E*` for etched.
  - Bulk edit also takes `*A*` for alter and `#tag` / `#!globaltag`.
  - Sources: [MythicHub](https://mythichub.com/help/importing-decklists),
    [Jerakin primer gist](https://gist.github.com/Jerakin/24be913c6106546136c45d1d028f9af9),
    and Forge's [DeckRecognizer.java@68e4572](https://github.com/Card-Forge/forge/blob/68e457200646b9602187ac3ddb2b8f73d8a63409/forge-core/src/main/java/forge/deck/DeckRecognizer.java),
    which treats `*F*` as foil and `*E*` as etched.
  - [Forge issue 11005](https://github.com/Card-Forge/forge/issues/11005) shows
    Moxfield's export writing `*F*`. PARSER/OBSERVED-ish.
- **Archidekt:** `1x Name (setcode) *F* [Cat1,Cat2{noDeck}{noPrice}{top}]`.
  Already observed in the Archidekt sandbox: see
  [archidekt-import-shapes.md](archidekt-import-shapes.md). This repo's
  fixtures (`crates/ichormoon-gauntlet/cli/tests/fixtures/*.txt`) are real
  Archidekt exports.
  - **UNVERIFIED:** `*E*` and a collector number after `(set)` in Archidekt
    text.
- **MTGGoldfish text:** `4 Name [SET]` with `(F)`. PARSER, Forge only.

---

## Bonus formats seen along the way (OBSERVED in MCH round-trips)

- **TopDecked:**
  - Header: `QUANTITY,"NAME",SETCODE,"SETNAME","COLLECTOR NUMBER",FINISH,PRICE,RARITY,ID,ACQUIRED DATE,ACQUIRED PRICE,LANG,PRICE SALE,SIGNING,ALTERATION,CONDITION,NOTES,TAGS`.
    Some header cells are quoted and some are not.
  - `ID` is the Scryfall UUID.
  - `FINISH` is `nonfoil` / `foil` / `etched`.
  - `CONDITION` is lowercase words (`near mint`).
  - The date looks like `Fri May 15 2026 16:43:29 GMT+0200 (Central European Summer Time)`.
  - Source: `MCH/Tests/topdecked-real-export.csv`.
- **Cardmarket stock export:**
  - Semicolon-delimited header:
    `idProduct;groupCount;price;idLanguage;condition;isFoil;isSigned;isAltered;isPlayset;isReverseHolo;isFirstEd;isFullArt;isUberRare;isWithDie`.
  - Language and condition are integers.
  - The printing key is `idProduct`, which maps to Scryfall `cardmarket_id`.
  - Source: `MCH/Tests/cardmarket-real-export.csv`.

---

## Cross-format observations for the importer

1. **Detect the format by header set, not order.** ManaBox, Deckbox,
   Archidekt, the TCGplayer app and Delver all change or let users pick
   columns.
   - Strip a first line that is `sep=,` or `"sep=,"`, and honour the separator
     it names.
   - Expect CRLF.
   - Never trust the order.
2. **Printing-key preference:**
   1. Scryfall ID (ManaBox, Archidekt, Helvault, TopDecked, Deckbox when
      selected, MTGGoldfish when filled).
   2. TCGplayer Product ID (TCGplayer app, Deckbox `TcgPlayer ID`, Delver
      option).
   3. Set code + collector number. Safe only for Moxfield, ManaBox and
      Archidekt, which use Scryfall's vocabulary.
   4. Set name + name. The last resort for Dragon Shield, Deckbox and
      TCGplayer, whose codes and numbers are their own.
3. **Set-code vocabularies:**

   | Vocabulary | Who |
   |---|---|
   | Scryfall | Moxfield (lowercase), Archidekt (lowercase), ManaBox (UPPER), Helvault, TopDecked |
   | Mostly Scryfall plus proprietary | Dragon Shield (UPPER), TCGplayer (UPPER) |
   | Gatherer/internal | Deckbox, MTGGoldfish (MTGO codes) |

4. **Double-faced names:** most formats write `A // B`. The exceptions:
   - Dragon Shield web writes the front face only for transform and MDFC.
   - The Dragon Shield app writes each face as its own row.
   - TCGplayer `Simple Name` keeps only the first half of a split card.
   - MTGGoldfish probably writes the front face only.

   Resolve either form against Scryfall `card_faces[0].name`.
5. **Tokens:** Dragon Shield decorates them (`X Token`, `Beast Token (4/4)`,
   `Morph Creature`). Deckbox files them under `Extras: <set>` with `ex_NNN`
   codes. Everyone else uses plain Scryfall token names in `t<set>` sets.
6. **Etched:** ManaBox, Moxfield, Archidekt and TopDecked write `etched`;
   Helvault writes `etchedFoil`; MTGGoldfish documents `FOIL_ETCHED`. Deckbox,
   TCGplayer, Cardmarket, and possibly Dragon Shield collapse it to foil.
7. **Location columns:**
   - ManaBox: `Binder Name` + `Binder Type` (`binder` / `list` / `deck`).
   - Dragon Shield: `Folder Name`.
   - Sorted: `List Type` + `List Name` (+ `Board`).
   - Moxfield, Archidekt, Deckbox, TCGplayer, MTGGoldfish, Helvault: none.
   - Delver: `lists.name` in `.dlens`. The CSV depends on the preset.
8. **Encoding:** every sample was UTF-8 without a BOM. Deckbox historically
   wanted ANSI on *import* (forum 2018). Delver's EchoMTG preset writes
   locale-formatted prices (`€ 1,77`).
