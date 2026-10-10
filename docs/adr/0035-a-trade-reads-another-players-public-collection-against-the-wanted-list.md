# A trade reads another player's public collection against the wanted list

Meldweb Curator answers *what could this player bring me* at `/trade?with=<login>`: the cards on the wanted list ([ADR-0034](0034-the-wanted-list-is-wanted-toml-and-what-the-decks-lack-is-derived.md)) that the other player's `collection.toml` ([ADR-0023](0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md)) holds, and the place each copy is in, so they can pull them before the two meet. It goes one way: what they want of the user's collection is not asked yet.

- **Their collection is their Magic repo's, read without the login.** The app's token reaches only the repo it is installed on ([ADR-0021](0021-curator-owns-one-fixed-name-repo-and-saves-by-itself.md)), and GitHub's docs make no exception for public repos. So `<login>/mtg` is read through the Contents API with no token, which only a public repo answers. A private one reads as no repo at all, and the page says that it has to be public. Nothing is written to either repo.
- **What is wanted is short copies, by name.** A hand want is short what it asks less the copies owned; the decks are short what *missing from decks* says; a card short both ways is short the sum. Any printing or finish of theirs answers it, and the page names the copy they hold.
- **A trade can be for one deck.** `&deck=<path>` wants only what that deck lacks against the whole collection, as if no other deck or hand want asked for a copy. That answers *what would building this one take from them* for a deck that is only an idea yet, which is a deck file like any other. Each deck links to it.
- **Their copies outside a deck go first.** Binders, boxes and unsorted copies are taken in file order, and copies sleeved in one of their decks only after, never more than are wanted. The list is laid out by their place, as they would walk to it, and copies as text to send them.
- **The matching is `chip-decklist`'s `trade` module**, beside `wanted`, so the browser groups and prints but never decides which card is which.

## Considered Options

- **Pasting their export (ManaBox, Moxfield…).** Not now. It would cover players who do not use Curator, through the collection import's reader ([ADR-0029](0029-a-collection-import-is-read-by-header-and-a-printing-is-pinned-only-where-scryfall-agrees.md)), and fits beside the login when someone asks for it.
- **Reading their repo with the user's token.** Rejected: it works only where the app is installed, which for someone else's repo it is not.
- **Both ways at once.** Deferred: their `wanted.toml` is in the same public repo, so the other direction is the same module run with the files swapped.
- **Offering only copies outside their decks.** Rejected: a sleeved copy can still be traded, so it is listed, last.
