import { beforeAll, describe, expect, it } from "vitest";
import { newDeck, setDeckMeta } from "../deck";
import { COLLECTION_PATH } from "./collection";
import { commitMessageFor } from "./repoFile";
import { SETTINGS_PATH } from "./settings";
import { loadWasm } from "./testkit";

beforeAll(loadWasm);

describe("a commit message", () => {
  it("is the diff of the file kind the path is", () => {
    const deck = newDeck("Burn", "modern");
    expect(
      commitMessageFor(
        "decks/burn.deck.toml",
        deck,
        setDeckMeta(deck, "Burn!"),
      ),
    ).toBe('burn: name: "Burn" → "Burn!"');
    expect(
      commitMessageFor(
        COLLECTION_PATH,
        "",
        "cards = []\n\n[places]\nBulk = {}\n",
      ),
    ).toMatch(/^collection: /);
    expect(
      commitMessageFor(
        SETTINGS_PATH,
        "",
        '[printings]\nrank = [{ avoid = "is:digital" }]\n',
      ),
    ).toMatch(/^meldweb\.toml: /);
  });

  it("refuses a path that is not a deck, the collection or the settings", () => {
    expect(() => commitMessageFor("VERSION", "1\n", "2\n")).toThrow(
      "VERSION is not a file Curator edits",
    );
    expect(() => commitMessageFor("decks/notes.md", "", "x")).toThrow();
  });
});
