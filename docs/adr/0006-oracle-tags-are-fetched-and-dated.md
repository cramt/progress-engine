# Oracle tags are fetched from Scryfall at sync time and dated

Facts that cannot be derived from card data, such as whether a land enters tapped, are asked for rather than guessed. `sync` fetches a fixed list of oracle tags (`STANDARD_TAGS`) and records them, with the date, in the index header. That header is the vocabulary authority: a tag the index never fetched is refused by name, because *nobody asked about that tag* and *no card is in it* are the same empty result and very different facts ([#41](https://github.com/cramt/progress-engine/issues/41), [#50](https://github.com/cramt/progress-engine/issues/50)). Keywords are derived from the cards, so an unknown keyword is refused against the card pool instead ([#53](https://github.com/cramt/progress-engine/issues/53)). A tag that fails to fetch costs that tag, not the whole download.

## Considered Options

- **Regex over oracle text.** Rejected: a Temple and a shockland look the same to it.
- **A hand-curated list of land cycles.** Rejected.
- **Fetching live during a run** ([#15](https://github.com/cramt/progress-engine/issues/15)). Rejected: an answer that depends on network reachability is not a test.

See [VISION.md: Ask, don't guess](../../VISION.md#ask-dont-guess).
