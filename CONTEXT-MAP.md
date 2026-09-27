# Context Map

progress-engine is a workspace of products that share a name family and, for now, little else. Each context keeps its own glossary. [VISION.md](VISION.md) is the product authority for Ichormoon Gauntlet; where a glossary and VISION.md disagree, VISION.md wins and the glossary is fixed. Architectural decisions live in [docs/adr/](docs/adr/).

## Contexts

- [Ichormoon Gauntlet](crates/ichormoon-gauntlet/CONTEXT.md): the draw-probability engine. A decklist and a criteria file in, exact probabilities out.
- [Reality Chip](crates/reality-chip/CONTEXT.md): the Magic-aware and Magic-free core Gauntlet stands on: card data, Scryfall query syntax, decklist parsing, and the hypergeometric walk.
- **Meldweb Curator** (`crates/meldweb-curator/`): the deck editor. A decklist in a git repo is the deck, and the editor is a browser front end that commits to it, modelled on Archidekt's editor ([docs/research/archidekt-editor.md](docs/research/archidekt-editor.md)). A scaffold so far, with no glossary yet.
- **Gitaxian Probe** (`crates/gitaxian-probe/`): card scanning via Delver X's recognition engine. An early MVP that does not work end to end yet, so it has no glossary or ADRs; its own README.md and FINDINGS.md are the authority on it.

## Relationships

- **Meldweb Curator → Reality Chip**: the editor runs `chip-decklist` compiled to wasm, so the browser and Gauntlet agree on what a decklist is. It is the second consumer of `chip-decklist` only; it uses nothing from `chip-scryfall` yet.
- **Ichormoon Gauntlet → Reality Chip**: Gauntlet was Reality Chip's first consumer. It reads the card index and query parser from `chip-scryfall`, decklists from `chip-decklist`, and walks compositions with `chip-stats`.
- **Reality Chip stats ↔ Magic**: `chip-stats` knows populations, groups, draws and removals, and nothing about Magic. Keep it that way ([ADR-0012](docs/adr/0012-tutors-are-deterministic-removals.md)).
- **Gitaxian Probe ↔ everything else**: none yet. The probe reads Delver's own catalogue, and the expected join key to the rest of the family is `scryfall_id`.

## Words that cross contexts

The same word means different things in different contexts. Qualify it when there is any doubt.

- **engine**: in Gauntlet, the exact engine or the sampler; in the probe, Delver X's blob or the Rust `Engine` around it.
- **oracle**: Scryfall's oracle text, oracle cards and oracle tags; separately, the sampler as the cross-check on the exact engine.
- **card**: in Reality Chip, an oracle card keyed by name; in the probe, a catalogue *printing*.
- **tier**: in Gauntlet, an effect tier; in the probe, a model tier (alpha, lambda, gamma).
- **artifact**: the Magic card type. The probe's downloaded files are "artefacts" in prose.
