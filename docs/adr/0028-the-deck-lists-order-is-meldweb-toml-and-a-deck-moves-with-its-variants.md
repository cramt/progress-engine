# The deck list's order is `meldweb.toml`'s, and a deck moves with its variants

The deck list was in name order, variants after their parent, and the user wants the decks they play first, in the order they choose, by dragging them.

- **The order is a list of paths in `meldweb.toml`.** `[decks] order = ["decks/loam.deck.toml", …]`. A path is a deck's identity and never follows a rename ([ADR-0021](0021-curator-owns-one-fixed-name-repo-and-saves-by-itself.md)), so a rename keeps a deck's place. A deck the list does not name goes after the decks it does, by name, so a new deck needs no write and a repo that never ordered its decks reads as before. A path for a deck since deleted is ignored and drops out at the next move.
- **One drag is one commit.** Dropped, the whole order is written at once, as a heart is ([ADR-0027](0027-a-cards-own-printing-is-a-pin-rule-and-an-added-card-takes-the-best.md)), with a subject naming the deck moved: `meldweb.toml: move loam to 1 of 5`. A file that moved on GitHub meanwhile gets the move made again on what is there. The settings page writes the order back as it found it.
- **A deck carries its variants, and a variant stays in its family.** Variants still follow their parent ([ADR-0024](0024-a-decks-past-is-gits-and-a-variant-is-a-deck.md)): a deck dragged takes them along, a variant moves only among its siblings, and a deck dropped on another family's variant goes beside that family.
- **Not only by mouse.** Alt+arrows on a focused tile and *Move earlier* / *Move later* in its menu move it one place among its siblings, which is also how a phone, with no HTML drag, orders the list.

## Considered Options

- **An `order` in each deck file.** Rejected: one drag would rewrite every deck between the two places, a commit to each, and a deck's file is the deck, not the list's layout.
- **The browser's storage.** Rejected: the repo is the user's data everywhere else, and an order kept in one browser is lost on the next.
- **Free placement, a variant anywhere.** Rejected: a variant beside its parent is what makes the list read as families.
