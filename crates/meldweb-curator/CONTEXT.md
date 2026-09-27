# Meldweb Curator

The deck editor: a browser front end over decks kept in git, modelled on Archidekt's editor. What a deck *is* belongs to [Reality Chip](../reality-chip/CONTEXT.md) and [ADR-0020](../../docs/adr/0020-decks-are-toml-with-typed-categories.md); this file pins the words for where decks live and how they change.

## Where decks live

**Magic repo**:
The one GitHub repository per user that Curator reads and writes, always named `mtg`. It holds the user's decks now and their collection later, and nothing Curator does reaches outside it.
_Avoid_: deck repo, decklists repo, storage

**Repo version**:
The integer in the Magic repo's root `VERSION` file, naming the layout and schema the repo follows. Curator refuses a repo newer than it knows and migrates an older one before editing.
_Avoid_: schema version, format version

**Deck**:
One `decks/*.deck.toml` file in the Magic repo. Its path is its identity; the `name` inside it is what the user sees and may change without the path following.
_Avoid_: list, decklist (for the file)

## Changing a deck

**Edit**:
One change to a deck's text through `chip-decklist`, such as a move, a quantity or a printing. Undo steps back one edit.

**Save**:
The commit that carries a deck's pending edits to the Magic repo. Curator makes it by itself once the deck has been idle ten seconds, or at once when the page is being closed or hidden. The user never saves by hand.
_Avoid_: sync, push

**Conflict**:
A save refused because the deck changed on GitHub since Curator loaded it. Saving stops until the user chooses to reload or overwrite; nothing is merged or lost silently.

**Import** / **Copy as Archidekt**:
Archidekt text into a deck, and a deck out as Archidekt text. The Archidekt text is read and written the way Archidekt itself reads it ([archidekt-import-shapes.md](../../docs/research/archidekt-import-shapes.md)).
_Avoid_: export (unqualified)
