// The committed decks, as the mock Magic repo starts in dev. Imported only
// when VITE_MOCK_GITHUB is set, so the real build does not carry them.
import lantern from "../../../../../decks/lantern.deck.toml?raw";
import loam from "../../../../../decks/loam.deck.toml?raw";

export const files: Record<string, string> = {
  VERSION: "1\n",
  "decks/lantern.deck.toml": lantern,
  "decks/loam.deck.toml": loam,
};
