// Uses a Scryfall term the parser does not support. It must refuse by name
// rather than silently matching nothing.
criterion("unsupported", (t) => t(0).count('power>=3') >= 1, { atLeast: 0.1 });
