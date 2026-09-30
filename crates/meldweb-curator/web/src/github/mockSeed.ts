// The committed decks and a small collection, as the mock Magic repo starts
// in dev. Imported only when VITE_MOCK_GITHUB is set, so the real build does
// not carry them.
import lantern from "../../../../../decks/lantern.deck.toml?raw";
import loam from "../../../../../decks/loam.deck.toml?raw";

export const files: Record<string, string> = {
  VERSION: "1\n",
  "decks/lantern.deck.toml": lantern,
  "decks/loam.deck.toml": loam,
  "collection.toml": `cards = [
  { printing = "moc/94", at = "Lantern" },  # Rashmi and Ragavan
  { name = "Sol Ring", qty = 3, at = "Bulk" },
  { name = "Lightning Bolt", qty = 4, at = "Trade binder" },
  { printing = "2xm/270", finish = "foil", at = "Trade binder" },  # Mana Crypt
  { name = "Island", qty = 40 },
]

[places]
Bulk = {}
Lantern = { deck = "decks/lantern.deck.toml" }
"Trade binder" = {}
`,
};
