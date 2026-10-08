import { beforeAll, describe, expect, it } from "vitest";
import { newDeck, setDeckMeta } from "../deck";
import { COLLECTION_PATH } from "./collection";
import { commitMessageFor, openFile } from "./repoFile";
import { createSaveStore } from "./save";
import { SETTINGS_PATH } from "./settings";
import { loadWasm, mockConnection, seedDecks } from "./testkit";

beforeAll(loadWasm);

const LANTERN = "decks/lantern.deck.toml";

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

describe("opening a file", () => {
  it("reads it only once the save in flight to it has landed", async () => {
    const { api, repo, mock } = mockConnection({ files: seedDecks() });
    const loaded = mock.file(LANTERN);
    if (!loaded) throw new Error("seeded");
    const store = createSaveStore({ api, repo, path: LANTERN, ...loaded });
    const renamed = setDeckMeta(loaded.text, "Lantern Control");
    store.edit(renamed);
    const saving = store.flush();
    const opened = await openFile(api, repo, LANTERN);
    await saving;
    store.dispose();
    expect(opened).toEqual(mock.file(LANTERN));
    expect(opened?.text).toBe(renamed);
  });

  it("is null for a file the repo does not have", async () => {
    const { api, repo } = mockConnection({ files: seedDecks() });
    expect(await openFile(api, repo, SETTINGS_PATH)).toBeNull();
  });
});
