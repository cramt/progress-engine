# A proxy sheet is a list of printings, and a deck is that list less what it says

Phyrexian Metamorph prints proxies. Its core takes a list of printings and makes the PDF; everything else is a way of writing that list.

- **The core is a list of printings.** `metamorph printings set/num …` is the whole tool. A printing named twice prints twice.
- **The sheet knows the size of a card and nothing else.** `metamorph-sheet` takes JPEGs and a layout (paper, gap, margin, cut marks) and lays them out at 63×88 mm. It does no I/O, so Curator can call it from wasm without a second implementation.
- **A deck is the list worked out from a `.deck.toml`.** Every line needing a physical copy, in the printing it names, which leaves out the maybeboard and what is set aside. `--no-basics` and `--minus <collection.toml>` say what else to leave out, and the run prints what it left out.
- **What a deck still needs is `chip-decklist`'s `supply`.** That is the call behind Curator's Sourcing view ([ADR-0036](0036-a-decks-sourcing-and-build-are-the-deck-against-its-copies.md)), so a copy in another printing or finish counts as owned, and so does a copy sleeved in another deck. With no collection, `supply` runs against an empty one, so the whole deck and the deck less the collection come from one path.
- **Images are Scryfall's JPEGs, embedded as they are.** `large` by default. `png` is not offered, because embedding it means decoding it.
- **A card Scryfall does not know stops the run** and names every such card. A missing proxy is something nobody notices until the deck is being sleeved.

## Considered Options

- **Subtracting the collection in the CLI by name.** Rejected: that would be a second definition of "owned" next to Sourcing's, and the two would drift.
- **Oracle-text proxies with no art.** Deferred. They would be another kind of sheet over the same list, chosen by a flag.
- **PNG for the extra resolution.** Deferred: 745×1040 against `large`'s 672×936, for a decoder and a file roughly three times the size.
