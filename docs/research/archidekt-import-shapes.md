# What Archidekt's import makes of every deck.toml shape

Question ([#108](https://github.com/cramt/progress-engine/issues/108)): for
each shape [ADR-0020](../adr/0020-decks-are-toml-with-typed-categories.md) can
express, which Archidekt text imports as the same thing, and what is lost where
Archidekt has no equivalent. Also: does the export need `{top}` on
non-commander cards?

## Method

Everything here was observed on 2026-09-27 in `archidekt.com/sandbox`, which
needs no account. Each probe reloaded the sandbox to an empty deck, typed a list
into **Import cards**, saved, and then read three things:

- **Size** in the deck header, which is the number Archidekt counts as the deck.
- The **Text / Categories** view, which groups each card under one category.
- **More → Export deck → Text → Download**, once with *Include out of deck
  cards* ticked and once without. That export is Archidekt's own serialisation,
  so it shows the flags it gave each category. One probe also downloaded the
  **MTGO .dek** export, which marks each card `Sideboard="true|false"`.

Most probes paired the shape with `4x Island`, so a shape counted toward the
deck when Size read 5 and did not when Size read 4. Screenshots were taken but
are not committed, because this repo is public and the screenshots show
Archidekt's UI.

## How Archidekt decides where a card is

These rules explain every probe below.

1. **The first category in the brackets is the card's primary.** The Categories
   view shows the card only under its primary. Later categories stay on the
   card: the export keeps them, and a second line for the same card merges
   into one entry that has the union of the categories (`1x Sol Ring [Ramp]` +
   `1x Sol Ring [Artifacts]` → `2x Sol Ring [Ramp,Artifacts]`).
2. **Only the primary decides where the card is.** `[Draw,Maybeboard]` and
   `[Recursion,Companion{noDeck}]` both counted toward the deck (Size 6). In
   the reverse order, `[Maybeboard,Draw]` and `[Companion{noDeck},Recursion]`,
   neither card counted (Size 4).
3. **`{top}` is the category's *Premier* flag.** The Edit category dialog
   describes Premier as "This category marks cards as being a Commander
   (Oathbreaker, Signature Spell, etc)". A card whose primary category is
   Premier is a commander: its group gets the crown and sorts first.
   `1x Lightning Bolt [Burn{top}]` made Lightning Bolt a commander.
   `1x Kenrith, the Returned King [Commander]`, with no `{top}`, gave a
   `Commander` group with no crown and Premier unticked, so Kenrith was not a
   commander.
4. **`{noDeck}` is the category's *In Deck* flag, cleared.** The card leaves
   Size and leaves the export when *Include out of deck cards* is unticked. The
   flag works on any category name (`Companion`, `Attractions`,
   `Sticker Sheet`, `Learnboard`).
5. **Two names are special, and both match only the exact spelling:**
   - `Maybeboard` gets `{noDeck}{noPrice}` when it is created, with no flag
     written. Archidekt exports it as `[Maybeboard{noDeck}{noPrice}]`.
   - `Sideboard` is a real board. A card with `Sideboard` as its primary is
     left out of Size, and the MTGO export marks it `Sideboard="true"`. The
     category still has In Deck ticked and no flag, so a card in it stays in
     the "in-deck only" text export. `sideboard` in lower case and
     `Sideboard Lessons` were ordinary categories that counted (Size 8 of 8).
6. **A card with no brackets gets an automatic category.** Archidekt chose the
   category itself: `4x Island` became `[Land]`, and `1x Swords to Plowshares`
   and `1x Counterspell` became `[Removal]`. Under a `# Heading`, the heading
   is the category instead.
7. **`# Heading` gives every following line that category as its primary**,
   until the next heading. Each line's own brackets come after it:
   `# Ramp` then `1x Sol Ring [Artifacts]` exported as `[Ramp,Artifacts]`.
   **`# Commander` sets Premier by itself**: the Kenrith line under it exported
   as `[Commander{top}]` and got the crown. The bracket form `[Commander]` does
   not do this (rule 3).

Other observations:

- `Commander{top}` counts toward Size. A 4-Island deck plus a commander read 5.
- The import box opens pre-filled with the current deck, and saving replaces
  the whole deck with the box's contents. Import is not an append.
- A line with no set code gets a printing that Archidekt chooses (Kenrith came
  back as `(plst) ELD-303`). The export always writes a printing.
  "All cards listed have been found!" was shown for every probe.

## Shape by shape

The *text* column is the line an export should write. "Counts" is whether the
card counted toward Size.

| deck.toml shape | Archidekt text | Archidekt result | Counts |
|---|---|---|---|
| commander | `1x Kenrith, the Returned King [Commander{top}]` | crowned Commander group, Premier | yes (right: the commander is one of the 100) |
| commander + untyped | `1x Kenrith, the Returned King [Commander{top},Ramp]` | crowned Commander group, keeps Ramp | yes |
| commander, no `{top}` | `1x Kenrith, the Returned King [Commander]` | a plain group named Commander, **not a commander** | yes |
| commander not first | `1x Kenrith, the Returned King [Ramp,Commander{top}]` | shown under Ramp with no crown | yes |
| companion | `1x Lurrus of the Dream-Den [Companion{noDeck}]` | Companion group, out of deck | no |
| companion, no flag | `1x Lurrus of the Dream-Den [Companion]` | an ordinary category | **yes (wrong)** |
| sideboard | `1x Counterspell [Sideboard]` | Sideboard board. MTGO export `Sideboard="true"` | no |
| sideboard, own name | `1x Negate [Learnboard{noDeck}]` | out of deck, but **not** the sideboard (MTGO `Sideboard="false"`) | no |
| maybeboard | `1x Brainstorm [Maybeboard]` | Maybeboard group, exported as `{noDeck}{noPrice}` | no |
| attractions | `1x Balloon Stand [Attractions{noDeck}]` | Attractions group, out of deck | no |
| attractions, no flag | `1x Balloon Stand [Attractions]` | an ordinary category | **yes (wrong)** |
| sticker sheet | `1x Ancestral Hot Dog Minotaur [Sticker Sheet{noDeck}]` | Sticker Sheet group, out of deck | no |
| sticker sheet, no flag | `1x Ancestral Hot Dog Minotaur [Sticker Sheet]` | an ordinary category | **yes (wrong)** |
| several untyped | `1x Sol Ring [Ramp,Artifacts]` | shown under Ramp. Artifacts kept on the card | yes |
| typed + untyped | `1x Path to Exile [Sideboard,Removal]` | sideboard. Removal kept | no |
| untyped + typed, wrong order | `1x Wrath of God [Removal,Sideboard]` | shown under Removal, **in the deck** | yes (wrong) |
| untyped in-deck, with a category | `1x Swords to Plowshares [Removal]` | Removal | yes |
| in-deck, no categories | `1x Swords to Plowshares` | Archidekt adds `[Removal]` | yes |
| qty > 1 | `4x Island [Lands]` | one entry, qty 4 | yes, 4 |
| `# Heading` | `# Sideboard` / `1x Counterspell` | same as `[Sideboard]` | no |

## What that means for "Copy as Archidekt"

- **Put the typed category first.** Only the primary decides where a card is, so
  a deck.toml card's typed category has to lead its brackets. The untyped
  categories follow in any order.
- **`{top}` goes on the commander category and nowhere else.** This answers the
  last part of #108. In Archidekt, `{top}` does not mean "premier category of
  this card". It means "this category marks commanders". On any other category
  it turns those cards into commanders (`[Burn{top}]`). deck.toml has no
  premier category, and Archidekt already gets a primary from bracket order, so
  non-commander cards need no marker.
- **Each not-in-deck type needs a word Archidekt reads:**
  - commander → `Commander{top}`
  - sideboard → `Sideboard` (exact case)
  - maybeboard → `Maybeboard` (exact case)
  - companion, attractions and sticker sheet → the category name with
    `{noDeck}`, e.g. `Companion{noDeck}`.

  Without the flag or the exact name, Archidekt counts the card in the deck.

## What is lost

- **Companion, attractions and sticker sheet are not boards in Archidekt.** It
  has no concept of them. `{noDeck}` keeps the card out of the count, but in
  Archidekt that is the same thing as a maybeboard: the MTGO export does not
  put a `{noDeck}` companion in the sideboard. A companion could instead be
  written `[Sideboard,Companion]`, which puts it on Archidekt's real sideboard
  and lets `chip-decklist` read it back as companion (it keeps the most
  specific type). That trade is for the export ticket
  ([#113](https://github.com/cramt/progress-engine/issues/113)) to decide.
- **A typed category's own name.** Archidekt recognises only
  `Sideboard`/`Maybeboard` by name, and commander by `{top}`. A deck.toml
  sideboard category named `learnboard` is either written as `Sideboard`
  (the name is lost), as `Learnboard{noDeck}` (Archidekt no longer knows it is
  a sideboard), or as `[Sideboard,learnboard]` (both survive, and
  `learnboard` comes back untyped).
- **Case.** Archidekt keeps a category name's case (`sideboard` stayed lower
  case), but the special names match only `Sideboard`/`Maybeboard`, so
  lower-case deck.toml keys must be written in title case to keep their
  meaning.
- **A card with no categories gains one.** Archidekt adds an automatic
  category of its own choosing, so a card with `in` empty comes back from
  Archidekt with a category it did not have.
- **Nothing is lost for:** commander (with `{top}`, first), several untyped
  categories, typed + untyped, qty > 1, maybeboard, sideboard named
  `Sideboard`.

## Where chip-decklist reads Archidekt differently

*Since [#114](https://github.com/cramt/progress-engine/issues/114) it no longer
does: `crates/reality-chip/decklist/src/archidekt.rs` reads the first category
only, and `tests/archidekt.rs` pins every row of both tables. `Attractions{noDeck}`
and `Sticker Sheet{noDeck}` now read back as their own types. What follows is
the state this research found.*

`Deck::from_archidekt` and `Entry::is_commander`/`is_outside`
(`crates/reality-chip/decklist/src/{deck,lib}.rs`) type a category by name
prefix (`commander`, `companion`, `sideboard`, `maybe`, case-insensitive) or
`{nodeck}`, and they look at **every** category on the line. Archidekt uses
exact names plus flags, and looks only at the **first** category. So the same
text can mean different things to each:

| Text | Archidekt | chip-decklist |
|---|---|---|
| `[Commander]` | not a commander | commander |
| `[Burn{top}]` | commander | untyped, in deck |
| `[Ramp,Commander{top}]` | not a commander | commander |
| `[Removal,Sideboard]` | in deck | sideboard |
| `[sideboard]`, `[Sideboard Lessons]` | in deck | sideboard |
| `[Companion]` (no flag) | in deck | companion |
| `[Attractions{noDeck}]`, `[Sticker Sheet{noDeck}]` | out of deck | `not-in-deck`, so attractions and sticker sheet read back less specifically |

An export that follows "What that means" above writes only text where the two
readers agree: typed category first, `Commander{top}`, `Sideboard`,
`Maybeboard`, `Companion{noDeck}`. The one exception is attractions and sticker
sheets, which come back as bare `not-in-deck` unless `archidekt_type` learns
`Attractions` and `Sticker Sheet`. `Sticker Sheet` needs to be matched in full,
not as a `sticker` prefix, because `is_outside` explains that "Sticker Package"
is an ordinary in-deck category.
