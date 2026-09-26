# Format-agnostic, and no legality checking

Draw probabilities need library size and composition, and both come from the decklist, so a format is never an input. A Commander legality checker was built and then removed ([#42](https://github.com/cramt/progress-engine/issues/42), `827acf6`), because it made a Modern list second-class for no mathematical reason. Zone annotations and never-in-library card types stay, because they change library size: that is maths, not rules. Card-level `f:` and `banned:` stay usable as selectors.

## Considered Options

- **Warn-and-continue legality checks** ([#24](https://github.com/cramt/progress-engine/issues/24)). Rejected.
- **A `--format` flag.** Rejected.

Whether pod modelling ([#20](https://github.com/cramt/progress-engine/issues/20), [#22](https://github.com/cramt/progress-engine/issues/22), [#23](https://github.com/cramt/progress-engine/issues/23)) still belongs in a format-agnostic tool is not decided.
