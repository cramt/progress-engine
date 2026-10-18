# Curator prints proxies and custom cards in the browser

Phyrexian Metamorph ([ADR-0038](0038-a-proxy-sheet-is-a-list-of-printings-and-a-deck-is-that-list-less-what-it-says.md)) and Mirage Mockery ([ADR-0039](0039-mockery-prints-custom-cards-and-never-a-real-one.md)) each have a library that does no I/O, so Curator runs them in the page rather than through a server.

- **A deck's proxies are a button on the deck page.** *Proxies…*, beside *Copy for…*, makes `metamorph deck`'s PDF: the whole deck or what the collection lacks, with or without basics. The count is `Supply::to_print`, one function the CLI calls too, so the deck page and `metamorph --minus` cannot disagree on what is owned. `metamorph-sheet` is compiled into `meldweb-wasm`. The page fetches Scryfall's `large` JPEGs, eight at a time, and hands the bytes over.
- **Custom cards are `cards/*.card.toml` in the Magic repo**, with their art beside them, Mockery's own format. The `/cards` page edits a file as its text beside the card drawn from it, and saves it as a commit, as a deck is saved. *Print sheet…* makes `mockery sheet`'s PDF, with the same paper, gap, margin and cut-mark fields as the proxies dialog.
- **Mockery is a wasm module of its own, `mockery-wasm`,** that only `/cards` fetches. Typst and its fonts are most of its size, and no other page needs them.
- ~~**The refusal of a real card asks Scryfall's search,** because the page has no card index and the browser's copy of Scryfall (ADR-0030) keeps no Oracle text. It asks for two things: a card by the same name, and the cards at the same cost whose text holds the custom card's longest word. A real card with the same text holds that word too, so the search leaves out no card the comparison could match. The comparison is `mockery-core`'s `real_card`, the same one the CLI runs over the index. A card that could not be checked is not printed, as a missing index stops `mockery`.~~ Dropped with Mockery's refusal (ADR-0039's amendment).

## Considered Options

- **Oracle text in the browser's copy of Scryfall.** Deferred. It would make the check work offline, but it would also grow every visitor's copy for one page.
- **Mockery inside `meldweb-wasm`.** Rejected: every page would download Typst.
- **Custom cards inside decks.** Deferred, as in the Mockery plan: a deck naming a custom card needs a format change in `chip-decklist` first.
