import { beforeAll, describe, expect, it } from "vitest";
import { addOwned, declarePlace } from "../collection";
import { COLLECTION_PATH, loadCollection } from "./collection";
import { createSaveStore } from "./save";
import { loadWasm, messages, watchedRepo } from "./testkit";

beforeAll(loadWasm);

describe("the collection file", () => {
  it("is empty while the repo has none, and its first save makes it", async () => {
    const { repo } = watchedRepo();
    const loaded = await loadCollection(repo);
    expect(loaded).toEqual({ text: "", sha: null });

    const store = createSaveStore({
      repo,
      path: COLLECTION_PATH,
      ...loaded,
    });
    const text = addOwned(
      declarePlace(loaded.text, "Lantern", "lantern"),
      { kind: "name", name: "Sol Ring" },
      "Lantern",
    ).text;
    store.edit(text);
    await store.flush();

    expect(store.getState()).toEqual({ status: "saved" });
    expect(await messages(repo, COLLECTION_PATH)).toEqual([
      "collection: +1 Sol Ring to lantern, +place lantern (lantern)",
    ]);
    expect(await loadCollection(repo)).toEqual({
      text,
      sha: (await repo.getFile(COLLECTION_PATH))?.sha,
    });
  });
});
