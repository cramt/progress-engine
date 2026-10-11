# A deck's sourcing and build are the deck against the copies that make it

A deck page has two views beside Stacks and Cost, both about the deck as physical cards. **Sourcing** (`?view=sourcing`) asks where each card is to come from: owned as listed, owned in another printing, a friend has it, or still to get. **Build** (`?view=build`) asks what the physical deck holds against the list: what to pull into it and from which place, what to take out, and which copies are already in it, to tick off while going through the deck. Both are one call to `chip-decklist`'s `supply` module, so the browser groups and prints but never decides which copy is which card. This is the view [ADR-0023](0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md) left for later: "comparing [a deck place] with the deck's list is a view built on top".

- **Every line needing a physical copy is matched to the collection's copies by name**, as the wanted list counts ([ADR-0034](0034-the-wanted-list-is-wanted-toml-and-what-the-decks-lack-is-derived.md)). The maybeboard and cards set aside need none.
- **A copy is exact or a stand-in.** It is exact when it is the printing the line names (any, for a line by name) in the line's finish. Anything else of the same card stands in: another printing, another finish, or a copy logged by name only, whose printing nobody wrote down.
- **Every line takes its exact copies before any line settles for a stand-in**, so one line's stand-in never takes another line's own copy. Within each pass, a copy already in the deck's place goes first, then loose ones (binders, boxes, unsorted, in file order), then copies sleeved in another deck. That order is the walk the user would make.
- **The deck is taken alone**, as a trade for one deck takes it ([ADR-0035](0035-a-trade-reads-another-players-public-collection-against-the-wanted-list.md)): the whole collection answers it, and a copy in another deck says where it is rather than being held back for that deck. `deck_copies` plays no part.
- **Copies in the deck's place that no line takes are extras**, to take out: not on the list, a copy too many, or a printing the list does not name.
- **Friends are a list of GitHub logins in `meldweb.toml`**, `[friends] logins = [...]`, added and removed on the Sourcing view and committed at once, as a heart is. What each one's public collection holds of what the deck is short of is the trade module's answer for that deck. Basics are never asked for.
- **A move on the Build view is one commit of `collection.toml`**, made at once and only on the collection as the page read it. If the file moved on GitHub since, nothing moves and the page shows the collection as it is now, because a move names lines by index and on another text that index may be another card. A move can be undone, by the same rule.
- **A deck without a place** gets a button that declares one named after it, since without one there is no telling what is sleeved.
- **The ticks on the Build view are the browser's**, kept per deck in local storage. They are about one sitting with the deck, not a fact about the collection.

## Considered Options

- **"Acquisition" for the sourcing view.** Rejected as a name: it is about more than buying, since most of a deck usually comes from the binder.
- **Friends typed into the page each time, as `/trade?with=` does.** Rejected. A deck asks the same people every time, and `meldweb.toml` is where the user's own choices already live.
- **Each friend allocated what is left after the one before.** Rejected. Who gets asked first is a choice between people that the page cannot make for the user. Each friend's copies are listed up to what is short, and the user picks.
- **Exact copies anywhere before stand-ins already in the deck.** This is what the module does. The other way, keeping whatever is sleeved, would hide a printing mismatch, and the Build view is how a mismatch gets checked. A stand-in in the deck is marked as one, and the Sourcing view can put the list on the owned printing.
- **Ticks in the collection file.** Rejected: a tick says "I saw it just now", not where a copy is.
