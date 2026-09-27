# Decks are TOML with declared, typed categories, and a card is named exactly once

The family's own deck format is a `.deck.toml`, parsed by `chip-decklist` into one `Deck` that Ichormoon Gauntlet and Meldweb Curator both read. Archidekt's text stays readable, as an import into the same `Deck`.

Archidekt's text says what a card *is* through naming conventions the parser has to guess: a commander is a category whose name starts with "Commander", a sideboard card one starting with "Sideboard", the premier category a `{top}` flag two categories can both carry. Every guess was a place for a deck to silently mean something else. Here each of those is structure:

```toml
name = "Izzet Lessons"
format = "modern"

cards = [
  { printing = "tla/46", in = ["learnboard", "self-bounce", "enemy-bounce", "tempo"] },  # Boomerang Basics
  { printing = "msc/183", qty = 4, in = ["tempo"] },  # Expressive Iteration
  { name = "Lightning Bolt", qty = 4, in = ["tempo"] },
]

[categories]
learnboard   = { type = "sideboard" }
self-bounce  = {}
enemy-bounce = {}
tempo        = {}
```

- **A card is named exactly once**: `printing = "set/num"` or `name = "..."`, never both, never neither. A printing already determines the card, so a line carrying a name as well could name one card and point at another. The name after a printing is a comment the writer regenerates, so it cannot disagree with anything.
- **Categories are declared**, and a card naming an undeclared one is refused, so a typo is an error rather than a new category.
- **A category may have a type**, from a tree the tool fixes:
  ```
  in-deck
  └── commander
  not-in-deck
  ├── sideboard
  │   └── companion
  ├── maybeboard
  ├── attractions
  └── sticker-sheet
  ```
  An untyped category is a label that says nothing about where the card is.
- **Where a card is follows from its categories**: the most specific type among them, or `in-deck` when none is typed. Lurrus in `companion` (companion) and `recursion` (untyped) is a companion that also shows under recursion. The typed categories must lie on one path of the tree; a card in both `commander` and `sideboard` is refused with its name.
- **A card is in as many categories as it needs**, with no premier. A view shows it under each; counts count it once.
- **`finish`** is `"foil"` or `"etched"`, absent for nonfoil. **`qty`** is absent for one. Unknown keys are refused, as in criteria files ([ADR-0002](0002-criteria-are-toml-data.md)).
- **`format` is metadata.** Gauntlet does not read it ([ADR-0005](0005-format-agnostic-no-legality-checking.md)): the tree decides what is in the library, not what is legal.

A printing identifies a card only through printing data, so resolving `msc/183` to Expressive Iteration offline needs the card index to carry printings. That is the price of naming a card once. `gauntlet sync` reads them from Scryfall's `default_cards` and files each after the cards as a line `printing:<set>/<number><TAB>"<card name>"`, the set lowercased; the header's `printings` counts them, and is absent from an index that carries none.

## Considered Options

- **Keep Archidekt's text as the format.** Rejected for the guessing above, and because it cannot say a card is a companion and a recursion piece at once.
- **Name and printing both on the line.** Rejected: two sources for one fact, which can disagree.
- **A premier category per card**, with the rest secondary. Rejected: which category a card is "really" in is a view's question, not the deck's.
- **User-defined types.** Rejected: a type exists to tell a tool where the card is, and a type the tool has never heard of cannot.
- **One `[[card]]` table per card.** Rejected: five lines a card, and a re-categorisation stops being a one-line diff.
