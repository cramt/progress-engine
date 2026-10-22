# Meldweb Curator

The deck editor: a browser front end over decks kept in git, modelled on Archidekt's editor. What a deck *is* belongs to [Reality Chip](../reality-chip/CONTEXT.md) and [ADR-0020](../../docs/adr/0020-decks-are-toml-with-typed-categories.md); this file pins the words for where decks live and how they change.

## Where decks live

**Magic repo**:
The one git repository per user that Curator reads and writes: a bare repo kept in the browser ([ADR-0041](../../docs/adr/0041-a-deck-is-a-line-of-history-and-the-repo-lives-in-the-browser.md)), made empty on the first visit, and moved between computers as a zip. It holds the user's decks and their collection, and nothing Curator writes reaches outside it.
_Avoid_: deck repo, decklists repo, storage

**Repo version**:
The integer in the Magic repo's root `VERSION` file, naming the layout and schema the repo follows: 2, a line per deck, since ADR-0041. Curator refuses a repo newer than it knows and migrates an older one before editing, all or nothing.
_Avoid_: schema version, format version

**Deck**:
A line of history of its own in the Magic repo ([ADR-0041](../../docs/adr/0041-a-deck-is-a-line-of-history-and-the-repo-lives-in-the-browser.md)): the branch `decks/<stem>/main` and any other branches `decks/<stem>/<branch>`, each tree holding `<stem>.deck.toml` and its Gauntlet criteria `<stem>.criteria.toml`. Only its `main` is the built deck that the collection, Build, Sourcing and the wanted list read.
_Avoid_: list, decklist (for the file)

**Stem**:
A deck's identity, slugged from its name when it is made and never following a rename: `lantern` names the deck `decks/lantern/main`, its file `lantern.deck.toml`, its snapshots `decks/lantern/*` and its deck place. The `name` inside the file is what the user sees.
_Avoid_: path, slug (for the identity)

**Branch**:
One of a deck's lines, `decks/<stem>/<branch>`: the same physical deck with a different list. Every deck has `main`, the built deck. Another branch starts at any commit of the deck's line (the branch open, a snapshot or a revision), opens at `?branch=<name>` from the picker beside the deck's name, and saves by itself like `main`; the collection, Build, Sourcing, proxies and the wanted list still read `main`. It **merges** into `main` (or `main` into it, to update it): a fast-forward when the branch merged into has no saves the other lacks, otherwise a merge commit with two parents whose message is the deck's changelog, `lantern: merge budget: +1 Sol Ring`. Where both changed the same lines the merge stops and shows them, until the user keeps one side's lines there; the rest of both is kept. Deleting a branch with saves `main` lacks asks first.
_Avoid_: variant, fork (that is another deck)

**Collection**:
The Magic repo's `collection.toml`: every card the user owns, and the place each copy is in ([ADR-0023](../../docs/adr/0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md)). A repo without the file owns nothing yet.
_Avoid_: inventory, card pool (that is every card in the card index)

**Place**:
Where owned copies physically are, declared in the collection: a trade binder, a bulk box, or a deck, for the copies sleeved in it. A copy is in one place at a time, unlike a deck's categories.
_Avoid_: category, location, bin

**Deck place**:
A place that names a deck by its stem, `deck = "lantern"`, so the copies in it are the ones in that deck. A repo-version-1 file named the deck's path, which reads as the same stem.

**Unsorted**:
Owned copies in no place yet.

**Wanted list**:
The Magic repo's `wanted.toml` and the page `/wanted` ([ADR-0034](../../docs/adr/0034-the-wanted-list-is-wanted-toml-and-what-the-decks-lack-is-derived.md)). It has two halves. The file holds cards wanted *by hand*, written like collection lines with no place. *Missing from decks* is worked out, never saved: what the decks hold more copies of than the collection, counted by name, with basics, maybeboard and set-aside cards left out. A deck and its forks count as one build. `deck_copies` in the file says whether each deck needs its own copies or one set moves between them.
_Avoid_: wishlist, shopping list, buy list

**Trade**:
The page `/trade?with=<login>` ([ADR-0035](../../docs/adr/0035-a-trade-reads-another-players-public-collection-against-the-wanted-list.md)): the copies another player's collection holds of what the wanted list is short of, by their place, read from their public Magic repo without the login. Copies sleeved in their decks come last. It goes one way, their collection against the user's wants. With `&deck=<path>` it is for one deck alone: what that deck lacks against the whole collection, built or not.
_Avoid_: match, swap

**Sourcing**:
A deck's view `?view=sourcing` ([ADR-0036](../../docs/adr/0036-a-decks-sourcing-and-build-are-the-deck-against-its-copies.md)): each card the deck needs a copy of, as owned as listed, owned in another printing (a **stand-in**), held by a friend, or still to get, each priced at the printing the list names. The deck is taken alone, against the whole collection.
_Avoid_: acquisition, shopping list

**Build**:
A deck's view `?view=build`: the deck's place against its list. It shows what to pull into the place from each binder, box or other deck, what to take out, what is missing, and a checklist of the copies already in it to tick while going through the physical deck. Each move is a commit of the collection.
_Avoid_: physical view, checkout

**Friends**:
GitHub logins in the settings, `[friends] logins`, whose public collections Sourcing reads to say who has what the deck is short of.
_Avoid_: contacts, trade partners

**Collection import**:
Another app's collection export (ManaBox, Moxfield, Dragon Shield…) or a text list, put into the collection as one edit ([ADR-0029](../../docs/adr/0029-a-collection-import-is-read-by-header-and-a-printing-is-pinned-only-where-scryfall-agrees.md)). Its binders become places. It adds to what is there, or replaces what the places it fills held, so importing the same export again changes nothing. A printing is pinned only where Scryfall agrees on the name; otherwise the card goes in by name, and the import says so.
_Avoid_: sync (nothing is kept in step with the other app)

**Settings**:
The Magic repo's `meldweb.toml`: Curator's own choices, which are the printing preference ([ADR-0026](../../docs/adr/0026-which-printing-comes-first-is-a-ranked-list-of-printing-queries.md)), the deck order and the friends; the preference is edited at `/settings` beside a preview that ranks one card's printings by the rules as they are typed. A repo without it gets the defaults, and the first change commits the file; a rule that does not read as a query holds the save back.
_Avoid_: config, preferences file

**Deck order**:
The order of the deck list, a list of deck stems in the settings, set by dragging a tile ([ADR-0028](../../docs/adr/0028-the-deck-lists-order-is-meldweb-toml-and-a-deck-moves-with-its-variants.md)). A deck not in it goes after those that are, by name; a deck moves with its forks, and a fork only among its siblings.
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

**Author**:
The name and email this browser's commits carry, set once in the settings and kept in the browser, outside the repo, as git keeps `user.name`. Until set, commits are by `Progress Engine <curator@localhost>`.
_Avoid_: user, account

**Save**:
The commit that carries a deck's (or the collection's) pending edits to the Magic repo. Curator makes it by itself once the deck has been idle ten seconds, or at once when the page is being closed or hidden. The user never saves by hand.
_Avoid_: sync, push

**Conflict**:
A save refused because the deck changed in the Magic repo since Curator loaded it: another tab or device saved it first. Saving stops until the user chooses to reload or overwrite; nothing is merged or lost silently. A sync has the same: a file changed here and on the remote in the same lines stops syncing until the user reloads it from the remote or overwrites the remote's.

**Remote**:
The one git repository on a host that the Magic repo syncs with, `origin` in its config, added in the settings ([ADR-0041](../../docs/adr/0041-a-deck-is-a-line-of-history-and-the-repo-lives-in-the-browser.md)). Optional: a repo with none is only in the browser and its zips. GitHub is the only host Curator logs in to so far; its remote is the user's `mtg` repo.
_Avoid_: backend, server, cloud

**Sync**:
Bringing the remote's commits into the Magic repo and pushing the Magic repo's to it, over git's own protocol, every branch and snapshot. It runs on opening a page, on coming back to one, and a few seconds after a save. Each branch on its own: only the remote moved, a fast-forward; both moved, a merge commit, merging each file by lines from the merge base; made or deleted on one side, made or deleted on the other. A remote still at repo version 1 is migrated in the browser, to the same commits a migration where it lives would make, and pushed there.
_Avoid_: save (a save is a local commit), backup

**Import** / **Copy for**:
Archidekt text into a deck, and a deck out as the text one tool imports: Archidekt, Cockatrice, Cardmarket or Tabletop Simulator, an entry each however alike their formats are. The Archidekt text is read and written the way Archidekt itself reads it ([archidekt-import-shapes.md](../../docs/research/archidekt-import-shapes.md)). Cockatrice gets each printing it can read back and the commander, companion and sideboard as `SB:` lines, having no commander zone; Cardmarket, for a wants list, one line a card by name with the copies summed; Tabletop Simulator's importer, every printing under `Commander`, `Deck` and `Sideboard` headings, the companion in the sideboard. The maybeboard and cards set aside go to none of them.
_Avoid_: export (unqualified)

**Playtest**:
The deck as it stands in the editor, saved or not, opened in Archidekt's playtester. The whole deck rides in the URL as Scryfall ids, the same way Archidekt's own sandbox opens its playtester, so nothing is uploaded to Archidekt. Only cards the game sees are sent: commanders to the command zone, sideboard and companions to the sideboard, attractions to the attraction deck.

## A deck's past

**Revision**:
A deck as one of the commits on its line that touched its file left it ([ADR-0024](../../docs/adr/0024-a-decks-past-is-gits-and-a-variant-is-a-deck.md), [ADR-0041](../../docs/adr/0041-a-deck-is-a-line-of-history-and-the-repo-lives-in-the-browser.md)). Every save makes one, and its commit message says what changed. `?at=<commit>` shows it in place of the deck.
_Avoid_: version (unqualified), checkpoint

**Take**:
To apply some of the changes between the deck and another revision or deck to the deck as it is now, as one edit that saves by itself and undoes. **Restore** is taking every change from a revision.
_Avoid_: merge, revert, cherry-pick

**Snapshot**:
A revision kept under a name the user chose, such as the list taken to an event. It is an annotated git tag under the deck's stem, `decks/<stem>/<slug>`, on a commit of the deck's line.
_Avoid_: tag (for what the user sees), save point

**Blame**:
For each card in a deck, the commit that last changed it and the one it came in at, with who made each and when ([ADR-0041](../../docs/adr/0041-a-deck-is-a-line-of-history-and-the-repo-lives-in-the-browser.md)). A card is followed by what it is, as the diff that writes commit messages pairs it, not by its text line. Its quantity, categories (a move to the sideboard or maybeboard included), printing and finish change it; the deck's name, description or categories declared do not. A card removed and added again came in at the second add. A change brought in by a merge is the commit on the side that made it, unless the merge settled a clash by dropping one side's change to the card, and a card unchanged since a fork is the deck it was forked from's. Edits not saved yet sit on top. A card's menu and details say it, `?view=blame` lists every card by its last change, newest first, and each commit opens that revision.
_Avoid_: history (that is the timeline), annotate

**Fork**:
A deck of its own whose line starts on a commit of another deck, such as a budget build: its history is that deck's up to the fork, as real ancestors ([ADR-0041](../../docs/adr/0041-a-deck-is-a-line-of-history-and-the-repo-lives-in-the-browser.md)). A fork of one's own deck is read from the history and written nowhere; either can take the other's changes, except the fork's name. A fork of **another player's** deck is made from Fork a deck from… on the deck list (their GitHub login or any git URL; `/?fork=<login>`, which the trade page links to): only their deck's line is fetched, over git and with no login, so their repo has to be public, and tracked under `refs/remotes/<host>/<path>/decks/<stem>/<branch>`, which is never pushed. Kept under their stem, the fork's `main` is their tip, same ids, same authors; under another stem, one commit on it names the files by the new stem. From a repo still at version 1, their `main` is fetched and migrated here, to the line their own migration will make. Repo version 1 called a fork of one's own deck a variant and named the parent with `variant_of`, which the migration takes out.
_Avoid_: variant (since repo version 2), copy, branch (that is the same deck)

**Upstream**:
The deck in another player's repo that a fork of it follows, `[upstream.<stem>]` in `meldweb.toml`: their repo's `url`, their `deck`'s stem, and a `branch` when it is not `main`. It travels with the repo, since git's own remote config does not survive a push, and a fork of one's own deck has none. The deck list shows the fork as *fork of alice's Lantern*; the deck shows *N new on alice's Lantern* once their line is fetched again (on opening the deck and on coming back to it, as often as a sync), a preview of what they changed since the two last agreed and of what main will become, and **taking their changes**: a fast-forward when the deck has not changed since it last took them, otherwise a merge commit with their tip as second parent, `lantern: merge alice's lantern: +1 Sol Ring`, a clash stopping until a side is picked, as a branch's merge does. *What you changed since* is the merge base against the deck's main. Unfollowing drops the record and keeps the history.
_Avoid_: remote (that is `origin`), parent (that is a fork of one's own deck)

