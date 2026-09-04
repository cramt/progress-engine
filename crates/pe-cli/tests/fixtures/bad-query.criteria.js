// A land cycle Scryfall curates by hand rather than storing as a field, so this
// crate declines it by name rather than guessing at it from oracle text.
criterion("tapland", (t) => t(0).count('is:tapland') >= 1);
