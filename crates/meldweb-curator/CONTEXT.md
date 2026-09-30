# Meldweb Curator

The deck editor: a browser front end over decks kept in git, modelled on Archidekt's editor. What a deck *is* belongs to [Reality Chip](../reality-chip/CONTEXT.md) and [ADR-0020](../../docs/adr/0020-decks-are-toml-with-typed-categories.md); this file pins the words for where decks live and how they change.

## Where decks live

**Magic repo**:
The one GitHub repository per user that Curator reads and writes, always named `mtg`. It holds the user's decks and their collection, and nothing Curator does reaches outside it.
_Avoid_: deck repo, decklists repo, storage

**Repo version**:
The integer in the Magic repo's root `VERSION` file, naming the layout and schema the repo follows. Curator refuses a repo newer than it knows and migrates an older one before editing.
_Avoid_: schema version, format version

**Deck**:
One `decks/*.deck.toml` file in the Magic repo. Its path is its identity; the `name` inside it is what the user sees and may change without the path following.
_Avoid_: list, decklist (for the file)

**Collection**:
The Magic repo's `collection.toml`: every card the user owns, and the place each copy is in ([ADR-0023](../../docs/adr/0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md)). A repo without the file owns nothing yet.
_Avoid_: inventory, card pool (that is every card in the card index)

**Place**:
Where owned copies physically are, declared in the collection: a trade binder, a bulk box, or a deck, for the copies sleeved in it. A copy is in one place at a time, unlike a deck's categories.
_Avoid_: category, location, bin

**Deck place**:
A place that names a deck's path, so the copies in it are the ones in that deck.

**Unsorted**:
Owned copies in no place yet.

## Changing a deck

**Edit**:
One change to a deck's or the collection's text through `chip-decklist`, such as a move, a quantity or a printing. Undo steps back one edit.

**Save**:
The commit that carries a deck's (or the collection's) pending edits to the Magic repo. Curator makes it by itself once the deck has been idle ten seconds, or at once when the page is being closed or hidden. The user never saves by hand.
_Avoid_: sync, push

**Conflict**:
A save refused because the deck changed on GitHub since Curator loaded it. Saving stops until the user chooses to reload or overwrite; nothing is merged or lost silently.

**Import** / **Copy as Archidekt**:
Archidekt text into a deck, and a deck out as Archidekt text. The Archidekt text is read and written the way Archidekt itself reads it ([archidekt-import-shapes.md](../../docs/research/archidekt-import-shapes.md)).
_Avoid_: export (unqualified)
