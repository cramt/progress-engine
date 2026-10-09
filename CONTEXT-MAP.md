# Context Map

progress-engine is a workspace of products that share a name family and, for now, little else. Each context keeps its own glossary. [VISION.md](VISION.md) is the product authority for Ichormoon Gauntlet; where a glossary and VISION.md disagree, VISION.md wins and the glossary is fixed. Architectural decisions live in [docs/adr/](docs/adr/).

## Contexts

- [Ichormoon Gauntlet](crates/ichormoon-gauntlet/CONTEXT.md): the draw-probability engine. A decklist and a criteria file in, exact probabilities out.
- [Reality Chip](crates/reality-chip/CONTEXT.md): the Magic-aware and Magic-free core Gauntlet stands on: card data, Scryfall query syntax, decklist parsing, and the hypergeometric walk.
- **Meldweb Curator** (`crates/meldweb-curator/`): the deck editor. A `.deck.toml` in a git repo is the deck ([ADR-0020](docs/adr/0020-decks-are-toml-with-typed-categories.md)), and the editor is a browser front end over it, modelled on Archidekt's editor ([docs/research/archidekt-editor.md](docs/research/archidekt-editor.md)). Its glossary is [crates/meldweb-curator/CONTEXT.md](crates/meldweb-curator/CONTEXT.md), where decks and the collection live and how they change ([ADR-0021](docs/adr/0021-curator-owns-one-fixed-name-repo-and-saves-by-itself.md), [ADR-0023](docs/adr/0023-the-collection-is-one-file-and-a-copy-is-in-one-place.md), [ADR-0029](docs/adr/0029-a-collection-import-is-read-by-header-and-a-printing-is-pinned-only-where-scryfall-agrees.md), [ADR-0024](docs/adr/0024-a-decks-past-is-gits-and-a-variant-is-a-deck.md)); what a deck and a collection are stays Reality Chip's.
- **Gitaxian Probe** (`crates/gitaxian-probe/`): card scanning via Delver X's recognition engine, natively and on the web. It has no glossary or ADRs; its engine README.md and FINDINGS.md are the authority on it.

## Relationships

- **Meldweb Curator → Reality Chip**: the editor runs `chip-decklist` compiled to wasm, so the browser and Gauntlet agree on what a decklist is, and the collection's format lives beside the deck's. It also runs `chip-scryfall`'s card objects and printing-query parser over its copy of Scryfall's bulk data ([ADR-0030](docs/adr/0030-curator-answers-card-facts-from-its-own-copy-of-scryfalls-bulk-data.md)), and over the `[printings]` rules.
- **Ichormoon Gauntlet → Reality Chip**: Gauntlet was Reality Chip's first consumer. It reads the card index and query parser from `chip-scryfall`, decklists from `chip-decklist`, and walks compositions with `chip-stats`.
- **Reality Chip stats ↔ Magic**: `chip-stats` knows populations, groups, draws and removals, and nothing about Magic. Keep it that way ([ADR-0012](docs/adr/0012-tutors-are-deterministic-removals.md)).
- **Meldweb Curator → Gitaxian Probe**: a dev-only spike. `MELDWEB_PROBE=1 pnpm dev` puts a Scan button in the editor that runs the probe's web host on the page and joins what it finds to the deck through `scryfall_id` ([probe-in-curator.md](docs/research/probe-in-curator.md)). No build or deploy carries it.
- **Gitaxian Probe ↔ everything else**: none. The probe reads Delver's own catalogue, and `scryfall_id` is the join key.

## Words that cross contexts

The same word means different things in different contexts. Qualify it when there is any doubt.

- **engine**: in Gauntlet, the exact engine or the sampler; in the probe, Delver X's blob or the Rust `Engine` around it.
- **oracle**: Scryfall's oracle text, oracle cards and oracle tags; separately, the sampler as the cross-check on the exact engine.
- **card**: in Reality Chip, an oracle card keyed by name; in the probe, a catalogue *printing*.
- **tier**: in Gauntlet, an effect tier; in the probe, a model tier (alpha, lambda, gamma).
- **artifact**: the Magic card type. The probe's downloaded files are "artefacts" in prose.
