# Context Map

progress-engine is a workspace of products that share a name family and, for now, little else. Each context keeps its own glossary. [VISION.md](VISION.md) is the product authority for Ichormoon Gauntlet; where a glossary and VISION.md disagree, VISION.md wins and the glossary is fixed. Architectural decisions live in [docs/adr/](docs/adr/).

## Contexts

- [Ichormoon Gauntlet](crates/ichormoon-gauntlet/CONTEXT.md): the draw-probability engine. A decklist and a criteria file in, exact probabilities out.
- [Reality Chip](crates/reality-chip/CONTEXT.md): the Magic-aware and Magic-free core Gauntlet stands on: card data, the copy of Scryfall that Gauntlet and Curator share, Scryfall query syntax, decklist parsing, and the hypergeometric walk.
- **Meldweb Curator** (`crates/meldweb-curator/`): the deck editor. A `.deck.toml` in a git repo is the deck ([ADR-0020](docs/adr/0020-decks-are-toml-with-typed-categories.md)), and the editor is a browser front end over it, modelled on Archidekt's editor ([docs/research/archidekt-editor.md](docs/research/archidekt-editor.md)). Its glossary is [crates/meldweb-curator/CONTEXT.md](crates/meldweb-curator/CONTEXT.md), where decks and the collection live and how they change ([ADR-0021](docs/adr/0021-curator-owns-one-fixed-name-repo-and-saves-by-itself.md), [ADR-0023](docs/adr/0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md), [ADR-0029](docs/adr/0029-a-collection-import-is-read-by-header-and-a-printing-is-pinned-only-where-scryfall-agrees.md), [ADR-0034](docs/adr/0034-the-wanted-list-is-wanted-toml-and-what-the-decks-lack-is-derived.md), [ADR-0035](docs/adr/0035-a-trade-reads-another-players-public-collection-against-the-wanted-list.md), [ADR-0024](docs/adr/0024-a-decks-past-is-gits-and-a-variant-is-a-deck.md)); what a deck and a collection are stays Reality Chip's.
- **Gitaxian Probe** (`crates/gitaxian-probe/`): card scanning via Delver X's recognition engine, natively and on the web. It has no glossary or ADRs; its engine README.md and FINDINGS.md are the authority on it.
- **Chancellor of the Spires** (`crates/chancellor-of-the-spires/`): the rules engine; so far the layer system over a battlefield state. The game is a typed Rust state machine, and what is true now is derived by ascent from Insight's trees, lowered into the engine's own continuous effects, one per layer ([ADR-0037](docs/adr/0037-chancellor-lowers-insights-trees-into-its-own-effects-and-applies-them-layer-by-layer.md)). The plan is [docs/research/rules-engine.md](docs/research/rules-engine.md).
- **Phyrexian Metamorph** (`crates/phyrexian-metamorph/`): proxies. A list of printings in, a PDF to print and cut out; a deck is that list worked out from a `.deck.toml`, less the basics or what the collection owns ([ADR-0038](docs/adr/0038-a-proxy-sheet-is-a-list-of-printings-and-a-deck-is-that-list-less-what-it-says.md)). Its README is the authority on it.
- **Mirage Mockery** (`crates/mirage-mockery/`, `mockery-core` and `mockery-cli`): custom cards from a `.card.toml` to PDF and PNG, laid out for printing at home, on a frame of its own ([ADR-0039](docs/adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)). The plan is [docs/research/mirage-mockery.md](docs/research/mirage-mockery.md).

## Relationships

- **Meldweb Curator → Reality Chip**: the editor runs `chip-decklist` compiled to wasm, so the browser and Gauntlet agree on what a decklist is, and the collection's format lives beside the deck's. It also runs `chip-scryfall`'s copy of Scryfall's bulk data in a worker, keeping it in the browser ([ADR-0030](docs/adr/0030-curator-answers-card-facts-from-its-own-copy-of-scryfalls-bulk-data.md), [ADR-0031](docs/adr/0031-gauntlet-and-curator-keep-one-copy-of-scryfall.md)) and handing it the `[printings]` rules' ranking, and its printing-query parser over those rules.
- **Ichormoon Gauntlet → Reality Chip**: Gauntlet was Reality Chip's first consumer. It reads the card index and query parser from `chip-scryfall`, decklists from `chip-decklist`, and walks compositions with `chip-stats`.
- **Reality Chip stats ↔ Magic**: `chip-stats` knows populations, groups, draws and removals, and nothing about Magic. Keep it that way ([ADR-0012](docs/adr/0012-tutors-are-deterministic-removals.md)).
- **Meldweb Curator → Gitaxian Probe**: a dev-only spike. `MELDWEB_PROBE=1 pnpm dev` puts a Scan button in the editor that runs the probe's web host on the page and joins what it finds to the deck through `scryfall_id` ([probe-in-curator.md](docs/research/probe-in-curator.md)). No build or deploy carries it.
- **Meldweb Curator → Phyrexian Metamorph and Mirage Mockery**: a deck page prints proxies through `metamorph-sheet` in `meldweb-wasm`, counted by the CLI's `Supply::to_print`, and `/cards` edits and prints `cards/*.card.toml` through `mockery-wasm`, its own module ([ADR-0040](docs/adr/0040-curator-prints-proxies-and-custom-cards-in-the-browser.md)).
- **Phyrexian Metamorph → Reality Chip**: a deck's proxies are `chip-decklist`'s `supply`, the call behind Curator's Sourcing view, so "owned" means the same in both. The sheet itself (`metamorph-sheet`) knows the size of a card and nothing else.
- **Chancellor of the Spires → Vivisurgeon's Insight**: the engine reads cards through Insight's parser and lowers each tree into its own effects. Where lowering a tree is awkward, that is feedback on the tree's shape.
- **Mirage Mockery ↔ Phyrexian Metamorph**: printing split at one line, custom cards and real ones. Mockery's sheet takes Metamorph's paper, gap, margin and cut-mark parameters so the two print alike, though it lays out vectors in Typst rather than JPEGs through `metamorph-sheet` ([ADR-0039](docs/adr/0039-mockery-prints-custom-cards-and-never-a-real-one.md)).
- **Mirage Mockery → Vivisurgeon's Insight**: Insight reads a custom card's rules text, and the lines it cannot read are reported. A warning, not a gate.
- **Gitaxian Probe ↔ everything else**: none. The probe reads Delver's own catalogue, and `scryfall_id` is the join key.

## Words that cross contexts

The same word means different things in different contexts. Qualify it when there is any doubt.

- **engine**: in Gauntlet, the exact engine or the sampler; in the probe, Delver X's blob or the Rust `Engine` around it; Chancellor is the rules engine.
- **oracle**: Scryfall's oracle text, oracle cards and oracle tags; separately, the sampler as the cross-check on the exact engine.
- **card**: in Reality Chip, an oracle card keyed by name; in the probe, a catalogue *printing*.
- **copy**: in Reality Chip, the copy of Scryfall, the one local Scryfall data Gauntlet and Curator share ([ADR-0031](docs/adr/0031-gauntlet-and-curator-keep-one-copy-of-scryfall.md)); in Curator's collection, one owned card in one place ([ADR-0023](docs/adr/0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md)).
- **effect**: in Gauntlet, what a card does to the counted zones (a tutor, a draw); in Chancellor, a continuous effect in one layer (CR 611, 613).
- **tier**: in Gauntlet, an effect tier; in the probe, a model tier (alpha, lambda, gamma).
- **artifact**: the Magic card type. The probe's downloaded files are "artefacts" in prose.
