# A card's own printing is a pin rule, and a card added to a deck takes the best printing

Most decks hold a Sol Ring, and its owner has one Sol Ring they want in all of them. [ADR-0026](0026-which-printing-comes-first-is-a-ranked-list-of-printing-queries.md) ranks printings by queries about what a printing *is*; it had no way to say "this one", and adding a card wrote a line by name, so every deck got whatever printing the user picked by hand afterwards.

- **A pin is a rule.** `{ prefer = '!"Sol Ring" set:sld cn:2417' }` in `[printings] rank` makes that printing the card's own. `chip-scryfall` learned the two terms it needs: `!"name"`, Scryfall's exact name (a card term, so Gauntlet's criteria have it too), and `cn:`, the collector number (a printing term, refused for a card by name like the others). A rule of exactly that shape reads as a pin. Curator writes pins above every other rule, so a pin beats them all, and only for its own card.
- **The page does not show a pin as a query.** The settings page lists pins as the cards they are, under *Your printings*, and the rule list holds only the rest. A heart on every printing, in a deck's printings grid and in the settings preview, pins or unpins it. From a deck, the heart commits `meldweb.toml` at once; on the settings page it is part of the draft, saved with it.
- **A card added to a deck takes its printing.** Quick add and the search's `+` write the new line by name at once, then set it to the card's pin, or to the printing the rules rank first, when Scryfall answers. Both are one undo. An add that only raises a line's quantity changes no printing, and an outage leaves the line by name. This replaces 0026's "it only orders": the ranking now also decides what a new line is. A pick is still the user's.
- **The collection is not ranked.** An owned copy is the printing in the binder, so adding to the collection by name stays by name.

## Considered Options

- **A table of pins beside the rules** (`[printings.pinned] "Sol Ring" = "sld/2417"`). Rejected by the user: a pin is a declared priority over printings like any rule, and [VISION.md](../../VISION.md) asks for one mechanism. That the page shows pins apart is a matter of the page, not of the file.
- **Pin, else by name** on add. Rejected: the rules already say which printing is best, and a deck of name-only lines shows Scryfall's pick instead.
- **Never write the printing on add.** Rejected: a deck file that does not say which Sol Ring cannot be the deck the user owns.
