# Curator owns one repo named `mtg`, a deck is a path in it, and saving happens by itself

Meldweb Curator logs a user in with a GitHub App installed on exactly one repository, and that repository is always called `mtg`. Decks are `decks/*.deck.toml` in it, and a later collection goes beside them. The root holds a `VERSION` file with one integer, the **repo version**. Curator refuses a repo newer than it knows, and migrates an older one in a single commit before editing anything.

A deck's **path is its identity**. The filename is slugged from the deck's name when the deck is created and never follows a rename; renaming a deck changes only `name` inside the file. So every ordinary save touches one file and is one Contents API `PUT` guarded by that file's blob `sha` ([github-login-static-site.md](../research/github-login-static-site.md)).

**Saving is not a user action.** Edits collect in the page and are committed once the deck has been idle ten seconds, or at once when the page is hidden or closing. The commit message is deterministic, generated in Rust from the semantic difference between the saved and the new `Deck`, for example `lantern: +1 Sol Ring, -1 Mind Stone, Sol Ring: ramp → draw`. That way the repo's log reads as the deck's changelog. When the `PUT` is refused because the file moved on GitHub, saving stops and the user chooses to reload or overwrite.

## Considered Options

- **Let the user pick any repo and any layout.** Rejected: a fixed name and layout mean every user's decks are found the same way, with no settings to lose. The repo version pays for changing them later.
- **Curator creates the repo.** Rejected if it needs a broader permission than Contents on one repo. Onboarding sends the user to GitHub's new-repo page with the name filled in, then to installing the app on that one repo.
- **Rename moves the file.** Rejected: it makes a rename a multi-file commit through a second API, for a filename nobody reads.
- **A commit per edit, or an explicit Save button.** Per edit runs into GitHub's 500 content writes an hour and fills the log with noise. A button is a thing to forget. An idle debounce is neither.
- **Merge a conflicting save by replaying the edits.** Deferred: conflicts are rare for one person, and refusing never loses anything silently.
- **A version inside each deck.toml.** Rejected for now: it would change ADR-0020's format for a problem that is about the repo's layout, not one file.
