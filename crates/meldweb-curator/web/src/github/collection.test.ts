import { beforeAll, describe, expect, it } from "vitest";
import { addOwned, declarePlace } from "../collection";
import { COLLECTION_PATH, loadCollection } from "./collection";
import { createSaveStore } from "./save";
import { loadWasm, mockConnection, seedDecks } from "./testkit";

beforeAll(loadWasm);

describe("the collection file", () => {
  it("is empty while the repo has none, and its first save makes it", async () => {
    const { api, repo, mock } = mockConnection({ files: seedDecks() });
    const loaded = await loadCollection(api, repo);
    expect(loaded).toEqual({ text: "", sha: null });

    const store = createSaveStore({
      api,
      repo,
      path: COLLECTION_PATH,
      ...loaded,
    });
    const text = addOwned(
      declarePlace(loaded.text, "Lantern", "decks/lantern.deck.toml"),
      { kind: "name", name: "Sol Ring" },
      "Lantern",
    );
    store.edit(text);
    await store.flush();

    expect(store.getState()).toEqual({ status: "saved" });
    expect(mock.file(COLLECTION_PATH)?.text).toBe(text);
    expect(mock.commits().at(-1)?.message).toBe(
      "collection: +1 Sol Ring to lantern, +place lantern (decks/lantern.deck.toml)",
    );
    expect(await loadCollection(api, repo)).toEqual({
      text,
      sha: mock.file(COLLECTION_PATH)?.sha,
    });
  });
});
