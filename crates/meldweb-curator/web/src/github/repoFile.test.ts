import { beforeAll, describe, expect, it } from "vitest";
import { newDeck, setDeckCover, setDeckMeta } from "../deck";
import { ConflictError } from "../repo/api";
import { COLLECTION_PATH } from "./collection";
import { deckFile } from "./decks";
import {
  commitEdit,
  commitMessageFor,
  openFile,
  settled,
  spotKey,
} from "./repoFile";
import { createSaveStore } from "./save";
import { SETTINGS_PATH } from "./settings";
import { fakeRepo, loadWasm, seedDecks } from "./testkit";

beforeAll(loadWasm);

// Lantern on its own line (ADR-0041).
const L = deckFile("lantern");
const LANTERN_TEXT = () => seedDecks()["decks/lantern.deck.toml"] ?? "";
const editLantern = (
  repo: Parameters<typeof commitEdit>[0],
  edit: Parameters<typeof commitEdit>[2],
) => commitEdit(repo, L.path, edit, L.branch);
const connect = (files: Record<string, string> = {}) => {
  const mock = fakeRepo({
    [spotKey(L.path, L.branch)]: LANTERN_TEXT(),
    ...files,
  });
  return { repo: mock.repo, mock };
};

describe("a commit message", () => {
  it("is the diff of the file kind the path is", () => {
    const deck = newDeck("Burn", "modern");
    expect(
      commitMessageFor("burn.deck.toml", deck, setDeckMeta(deck, "Burn!")),
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
      "VERSION is not a file Progress Engine edits",
    );
    expect(() => commitMessageFor("decks/notes.md", "", "x")).toThrow();
  });
});

describe("opening a file", () => {
  it("reads it only once the save in flight to it has landed", async () => {
    const { repo, mock } = connect();
    const loaded = mock.file(L.path, L.branch);
    if (!loaded) throw new Error("seeded");
    const store = createSaveStore({ repo, ...L, ...loaded });
    const renamed = setDeckMeta(loaded.text, "Lantern Control");
    store.edit(renamed);
    const saving = store.flush();
    const opened = await openFile(repo, L.path, L.branch);
    await saving;
    store.dispose();
    expect(opened).toEqual(mock.file(L.path, L.branch));
    expect(opened?.text).toBe(renamed);
  });

  it("is null for a file the repo does not have", async () => {
    const { repo } = connect();
    expect(await openFile(repo, SETTINGS_PATH)).toBeNull();
  });
});

describe("a one-shot edit", () => {
  const rename = (t: string | null) => setDeckMeta(t ?? "", "Lantern Control");
  const ours = (mock: ReturnType<typeof fakeRepo>) =>
    mock.commits().filter((c) => !c.outOfBand);
  const puts = (mock: ReturnType<typeof fakeRepo>) =>
    mock.calls().filter((r) => r.method === "PUT");

  it("commits the edit of the file as it is, with its diff's message", async () => {
    const { repo, mock } = connect();
    const made = await commitEdit(repo, L.path, rename, L.branch);
    expect(made).toEqual({ kind: "committed", ...mock.file(L.path, L.branch) });
    expect(mock.commits().map((c) => c.message)).toEqual([
      'lantern: name: none → "Lantern Control"',
    ]);
  });

  it("is made again on what GitHub has when the file moved under it, keeping the other change", async () => {
    const { repo, mock } = connect();
    const seeded = mock.file(L.path, L.branch)?.text ?? "";
    const covered = setDeckCover(seeded, { set: "cmr", num: "304" });
    const seen: (string | null)[] = [];
    const editing = editLantern(repo, (t) => {
      seen.push(t);
      if (seen.length === 1) mock.editElsewhere(L.path, covered, L.branch);
      return rename(t);
    });
    // Whatever reads the file next waits out the retry, not just the first try.
    await settled(spotKey(L.path, L.branch));
    expect(mock.file(L.path, L.branch)?.text).toBe(rename(covered));
    await editing;
    expect(seen).toEqual([seeded, covered]);
    expect(mock.file(L.path, L.branch)?.text).toBe(rename(covered));
    expect(ours(mock)).toHaveLength(1);
  });

  it("gives up with the conflict after three attempts, committing nothing", async () => {
    const { repo, mock } = connect();
    let n = 0;
    const edit = editLantern(repo, (t) => {
      n++;
      mock.editElsewhere(L.path, `${t}# moved ${n}\n`, L.branch);
      return rename(t);
    });
    await expect(edit).rejects.toBeInstanceOf(ConflictError);
    expect(n).toBe(3);
    expect(ours(mock)).toEqual([]);
  });

  it("writes nothing for an edit that changes nothing, or that throws", async () => {
    const { repo, mock } = connect();
    const file = mock.file(L.path, L.branch);
    expect(await editLantern(repo, (t) => t)).toEqual({
      kind: "unchanged",
      ...file,
    });
    expect(await editLantern(repo, () => null)).toMatchObject({
      kind: "unchanged",
    });
    await expect(
      editLantern(repo, () => {
        throw new Error("refused");
      }),
    ).rejects.toThrow("refused");
    expect(puts(mock)).toEqual([]);
  });

  it("creates a file the repo lacks, and is made again on one created meanwhile", async () => {
    const decks = '\n[decks]\norder = ["decks/loam.deck.toml"]\n';
    const order = (t: string | null) => `${t ?? ""}${decks}`;
    const absent = connect();
    await commitEdit(absent.repo, SETTINGS_PATH, order);
    expect(absent.mock.file(SETTINGS_PATH)?.text).toBe(decks);

    const raced = connect();
    const rules = '[printings]\nrank = [{ avoid = "is:digital" }]\n';
    const seen: (string | null)[] = [];
    await commitEdit(raced.repo, SETTINGS_PATH, (t) => {
      seen.push(t);
      if (seen.length === 1) raced.mock.editElsewhere(SETTINGS_PATH, rules);
      return order(t);
    });
    expect(seen).toEqual([null, rules]);
    expect(raced.mock.file(SETTINGS_PATH)?.text).toBe(order(rules));
  });

  it("asked for twice at once, makes the second on the first without a conflict", async () => {
    const { repo, mock } = connect();
    const cover = (t: string | null) =>
      setDeckCover(t ?? "", { set: "cmr", num: "304" });
    await Promise.all([
      commitEdit(repo, L.path, rename, L.branch),
      commitEdit(repo, L.path, cover, L.branch),
    ]);
    expect(puts(mock)).toHaveLength(2);
    expect(mock.file(L.path, L.branch)?.text).toBe(
      cover(rename(LANTERN_TEXT())),
    );
  });
});
