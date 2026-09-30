# The collection is one `collection.toml`, and each copy is in one place

Meldweb Curator keeps the cards a user owns in the Magic repo beside their decks ([ADR-0021](0021-curator-owns-one-fixed-name-repo-and-saves-by-itself.md)), as one file at its root, `collection.toml`. It is parsed and edited by `chip-decklist`'s `collection` module, as a deck is, so the browser never decides what a collection is.

```toml
cards = [
  { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan
  { name = "Sol Ring", qty = 3, at = "Bulk" },
  { printing = "2xm/270", finish = "foil", at = "Trade binder" },  # Mana Crypt
  { name = "Island", qty = 40 },
]

[places]
Bulk = {}
"Trade binder" = {}
Lantern = { deck = "decks/lantern.deck.toml" }
```

- **A card line is a deck's card line** ([ADR-0020](0020-decks-are-toml-with-typed-categories.md)): named once, by printing or by name, with `qty` absent for one and `finish` absent for nonfoil. The two files read alike, and share their line edits and the checks on a line.
- **Where a copy is, is a place, and it is in one.** A deck's categories are labels, and a card can carry many, because a category is a question about the deck. A collection's place is a physical location: a copy is in the trade binder or in the bulk box, never both. So a line has `at`, a single place, not `in`, a list. Copies of one card in two places are two lines.
- **A card with no `at` is unsorted**: owned, and not put anywhere yet. Quick add puts cards there unless told otherwise.
- **Places are declared** under `[places]`, and a line naming an undeclared one is refused, as a deck refuses an undeclared category. A place can be dropped only once it holds nothing.
- **A place can be a deck.** `deck = "<path>"` says the place is that deck, and its cards are the copies sleeved in it. Two places cannot be the same deck. The path is the deck's identity (ADR-0021), so renaming a deck does not break the link.
- **Moving is the edit that matters.** Moving all of a line changes its `at` in place, a one-line diff. Moving some copies leaves the rest where they were. Either way, the moved copies join a line that already holds the card with the same finish in the place they go to, so the file does not collect duplicate lines.
- **The changelog is about copies, not lines.** A save's commit message is generated in Rust, as a deck's is. It matches what left each place to what arrived in another before it calls anything an add or a removal: `collection: 2 Sol Ring: bulk → trade binder`, and not `+2 Sol Ring, -2 Sol Ring`.
- **A missing file is the empty collection.** No repo version bump or migration: an older Curator never reads the file, and the first save creates it.

## Considered Options

- **The collection as a `.deck.toml` whose categories are places.** Rejected. It would accept a copy in two places at once, and a deck's category types (commander, sideboard) mean nothing for a binder.
- **One file per place** (`collection/bulk.toml`). Rejected. Moving a card between places is the most common edit, and this makes it a two-file commit. ADR-0021 keeps every save to one file for the same reason.
- **Labels as well as places** (`for-trade`, `want-to-sell`). Deferred. Nothing asked for them yet. If they come, they are an `in` list beside `at`, and `at` stays single.
- **Deriving "in this deck" from the deck files.** Rejected as the source of truth. A deck can hold proxies, or cards not yet bought, and what is owned is not the same question as what is in a list. A deck place records which copies are physically in the deck, and comparing it with the deck's list is a view built on top.
- **Condition, language and price per line.** Deferred. Each is one more optional key on a line. Unknown keys are refused, so adding one is a format change made on purpose, not a typo accepted.
