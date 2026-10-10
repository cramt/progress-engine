# The wanted list is `wanted.toml`, and what the decks lack is derived

Meldweb Curator keeps a wanted list at `/wanted` with two halves. Cards wanted for no reason the decks give are written by hand into one file at the Magic repo's root, `wanted.toml`. Cards the decks hold more copies of than the collection ([ADR-0023](0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md)) are worked out each time the page opens and never written down. Both are `chip-decklist`'s `wanted` module, so the browser never decides which deck card is which owned card.

```toml
deck_copies = "shared"
cards = [
  { name = "The One Ring" },
  { printing = "ltr/451", finish = "foil" },  # The One Ring
]
```

- **A want is a collection line without `at`.** It is named by printing or by name, with `qty` and `finish`, and shares the collection's line edits. It has no place because it is not anywhere yet.
- **The derived half counts by name.** An owned copy of any printing, in any finish and any place, counts against a deck's card. Maybeboard and set-aside cards need no copy; everything else in a deck does, sideboard and companion included. Basic lands are left out, since few people log them and a deck holds as many as it likes.
- **How decks count is the user's, in the file.** `deck_copies = "each"` (the absent key) gives every deck its own copies, as when decks stay sleeved: a card in three decks needs three. `"shared"` moves one set between decks: the card needs as many as the deck wanting most of it. The setting is in `wanted.toml` because the page that shows the list is the page that saves it.
- **A deck and its variants are one build** ([ADR-0024](0024-a-decks-past-is-gits-and-a-variant-is-a-deck.md)). A budget variant is another way to build the same deck, not another deck on the shelf, so a family needs as many copies as its hungriest member, whichever way `deck_copies` reads.
- **The changelog is about copies**, as the collection's is: `wanted: +1 The One Ring, -2 Sol Ring (foil), deck copies: each → shared`.
- **A missing file wants nothing.** The first save creates it.

## Considered Options

- **Wants inside `collection.toml`.** Rejected. Every line edit works on the file's `cards` array, and a second array would mean threading its key through all of them. It would also turn a file that says what is owned into one that also says what is not.
- **Writing the derived half down.** Rejected. It would go stale with every deck save, and what a deck holds is already in the deck.
- **`deck_copies` in `meldweb.toml`.** Rejected for now. Only this page reads it, and putting it in the settings file would make one toggle a save of another file.
- **A "got it" button that moves a want into the collection.** Deferred. That is a save of two files at once, which no page does yet (ADR-0021). For now, adding the card to the collection updates the want's *owned* count, and the user removes the want.
