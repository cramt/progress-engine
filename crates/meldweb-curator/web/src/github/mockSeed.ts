// The committed decks and a small collection, as the mock Magic repo starts
// in dev. Imported only when VITE_MOCK_GITHUB is set, so the real build does
// not carry them.
import lantern from "../../../../../decks/lantern.deck.toml?raw";
import loam from "../../../../../decks/loam.deck.toml?raw";
import {
  addCard,
  commitMessage,
  parseDeck,
  removeCard,
  setCardCategories,
} from "../deck";
import type { SeedCommit } from "./mock";

const LANTERN = "decks/lantern.deck.toml";

export const files: Record<string, string> = {
  VERSION: "1\n",
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

/** The index of the card named by printing `id` (`set/num`) in `text`. */
function at(text: string, id: string): number {
  const deck = parseDeck(text);
  const card =
    deck.kind === "deck" &&
    deck.cards.find(
      (c) => c.card.kind === "printing" && `${c.card.set}/${c.card.num}` === id,
    );
  if (!card) throw new Error(`the seed deck has no ${id}`);
  return card.index;
}

const without = (text: string, id: string) => removeCard(text, at(text, id));
const rock = (text: string, name: string) =>
  addCard(text, { kind: "name", name }, ["Artifact Count"]);

/**
 * Lantern's last six weeks, so a fresh dev session has a past to look
 * through. It is built backwards from the committed list, which is where it
 * ends, and each step's message is the one a save would have written.
 */
export function history(now: number): SeedCommit[] {
  const day = 24 * 60 * 60 * 1000;
  const v4 = lantern;
  const v3 = setCardCategories(v4, at(v4, "cmr/304"), ["Draw"]);
  const v2 = rock(without(v3, "30a/282"), "Thought Vessel");
  const v1 = setCardCategories(
    without(v2, "mb2/157"),
    at(without(v2, "mb2/157"), "c18/122"),
    ["Interaction"],
  );
  const v0 = rock(
    without(without(v1, "peld/71p"), "rav/75"),
    "Commander's Sphere",
  );
  const versions = [
    { text: v0, daysAgo: 40 },
    { text: v1, daysAgo: 26 },
    { text: v2, daysAgo: 12 },
    { text: v3, daysAgo: 5 },
    { text: v4, daysAgo: 1 },
  ];
  // The hour varies so the timeline does not read as generated.
  return versions.map((v, i) => ({
    path: LANTERN,
    text: v.text,
    message: commitMessage(versions[i - 1]?.text ?? "", v.text, LANTERN),
    date: new Date(now - v.daysAgo * day + i * 37 * 60 * 1000).toISOString(),
  }));
}
