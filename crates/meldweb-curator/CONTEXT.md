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

**Collection import**:
Another app's collection export (ManaBox, Moxfield, Dragon Shield…) or a text list, put into the collection as one edit ([ADR-0029](../../docs/adr/0029-a-collection-import-is-read-by-header-and-a-printing-is-pinned-only-where-scryfall-agrees.md)). Its binders become places. It adds to what is there, or replaces what the places it fills held, so importing the same export again changes nothing. A printing is pinned only where Scryfall agrees on the name; otherwise the card goes in by name, and the import says so.
_Avoid_: sync (nothing is kept in step with the other app)

**Settings**:
The Magic repo's `meldweb.toml`: Curator's own choices, which are the printing preference ([ADR-0026](../../docs/adr/0026-which-printing-comes-first-is-a-ranked-list-of-printing-queries.md)), and the deck order; the preference is edited at `/settings` beside a preview that ranks one card's printings by the rules as they are typed. A repo without it gets the defaults, and the first change commits the file; a rule that does not read as a query holds the save back.
_Avoid_: config, preferences file

**Deck order**:
The order of the deck list, a list of deck paths in the settings, set by dragging a tile ([ADR-0028](../../docs/adr/0028-the-deck-lists-order-is-meldweb-toml-and-a-deck-moves-with-its-variants.md)). A deck not in it goes after those that are, by name; a deck moves with its variants, and a variant only among its siblings.
_Avoid_: sort order, pinned decks

**Copy of Scryfall**:
Reality Chip's [copy of Scryfall](../reality-chip/CONTEXT.md) as the browser keeps it ([ADR-0031](../../docs/adr/0031-gauntlet-and-curator-keep-one-copy-of-scryfall.md)): made on the first visit, kept in origin-private storage and refreshed once a day ([ADR-0030](../../docs/adr/0030-curator-answers-card-facts-from-its-own-copy-of-scryfalls-bulk-data.md)). It answers every card fact the editor asks (printings, prices, all printings of a card, search, quick add) in a worker; until it is ready, or when it cannot answer, Scryfall's API does. Pictures always come from Scryfall's CDN. A card a deck names only by name shows the printing the default printing preference ranks first, not the one the user's own rules would pick.
_Avoid_: index, cache, mirror

**Printing preference**:
The ordered `prefer`/`avoid` rules in the settings that decide which printing of a card is offered first, and which one a card added to a deck by name gets ([ADR-0027](../../docs/adr/0027-a-cards-own-printing-is-a-pin-rule-and-an-added-card-takes-the-best.md)). Each rule is a Scryfall query over one printing; the first rule that tells two printings apart decides, and the newest wins what none of them decides. A pick is always the user's.
_Avoid_: printing filter, default printing

**Pin**:
A card's own printing, its *favourite* on the page: a `prefer` rule shaped `!"Sol Ring" set:sld cn:2417`, written above every other rule, set and cleared with the heart on a printing. The settings page shows pins as cards under *Your printings*, never as queries.
_Avoid_: default printing

## Changing a deck

**Edit**:
One change to a deck's or the collection's text through `chip-decklist`, such as a move, a quantity or a printing, made to one card or to every card selected at once. Undo steps back one edit. A card added by name or by a scan joins the line that already holds it, which Rust finds from the names the browser has for the printings in the file: a name finds a line of its printing, a double-faced card is found by its front face, a scanned printing joins only a line of that printing, and a foil line is another card.

**Save**:
The commit that carries a deck's (or the collection's) pending edits to the Magic repo. Curator makes it by itself once the deck has been idle ten seconds, or at once when the page is being closed or hidden. The user never saves by hand.
_Avoid_: sync, push

**Conflict**:
A save refused because the deck changed on GitHub since Curator loaded it. Saving stops until the user chooses to reload or overwrite; nothing is merged or lost silently.

**Import** / **Copy for**:
Archidekt text into a deck, and a deck out as the text one tool imports: Archidekt, Cockatrice or Cardmarket, an entry each however alike their formats are. The Archidekt text is read and written the way Archidekt itself reads it ([archidekt-import-shapes.md](../../docs/research/archidekt-import-shapes.md)). Cockatrice gets each printing it can read back and the commander, companion and sideboard as `SB:` lines, having no commander zone; Cardmarket, for a wants list, one line a card by name with the copies summed. The maybeboard and cards set aside go to neither.
_Avoid_: export (unqualified)

**Playtest**:
The deck as it stands in the editor, saved or not, opened in Archidekt's playtester. The whole deck rides in the URL as Scryfall ids, the same way Archidekt's own sandbox opens its playtester, so nothing is uploaded to Archidekt. Only cards the game sees are sent: commanders to the command zone, sideboard and companions to the sideboard, attractions to the attraction deck.

## A deck's past

**Revision**:
A deck as one of the commits that touched its path left it ([ADR-0024](../../docs/adr/0024-a-decks-past-is-gits-and-a-variant-is-a-deck.md)). Every save makes one, and its commit message says what changed. `?at=<commit>` shows it in place of the deck.
_Avoid_: version (unqualified), checkpoint

**Take**:
To apply some of the changes between the deck and another revision or deck to the deck as it is now, as one edit that saves by itself and undoes. **Restore** is taking every change from a revision.
_Avoid_: merge, revert, cherry-pick

**Snapshot**:
A revision kept under a name the user chose, such as the list taken to an event. It is an annotated git tag under the deck's path, `decks/<stem>/<slug>`.
_Avoid_: tag (for what the user sees), save point

**Variant**:
A deck that names another as its parent with `variant_of`, such as a budget build. It is a deck file of its own, not a branch. Either can take the other's changes, except the variant's name and parent.
_Avoid_: branch, fork, copy
