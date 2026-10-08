import { beforeAll, describe, expect, it } from "vitest";
import { newDeck, setDeckCover, setDeckMeta } from "../deck";
import { ConflictError } from "./api";
import { COLLECTION_PATH } from "./collection";
import { commitEdit, commitMessageFor, openFile, settled } from "./repoFile";
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

describe("a one-shot edit", () => {
  const rename = (t: string | null) => setDeckMeta(t ?? "", "Lantern Control");
  const ours = (mock: ReturnType<typeof mockConnection>["mock"]) =>
    mock.commits().filter((c) => !c.outOfBand);
  const puts = (mock: ReturnType<typeof mockConnection>["mock"]) =>
    mock.requests().filter((r) => r.method === "PUT");

  it("commits the edit of the file as it is, with its diff's message", async () => {
    const { api, repo, mock } = mockConnection({ files: seedDecks() });
    const made = await commitEdit(api, repo, LANTERN, rename);
    expect(made).toEqual({ kind: "committed", ...mock.file(LANTERN) });
    expect(mock.commits().map((c) => c.message)).toEqual([
      'lantern: name: none → "Lantern Control"',
    ]);
  });

  it("is made again on what GitHub has when the file moved under it, keeping the other change", async () => {
    const { api, repo, mock } = mockConnection({ files: seedDecks() });
    const seeded = mock.file(LANTERN)?.text ?? "";
    const covered = setDeckCover(seeded, { set: "cmr", num: "304" });
    const seen: (string | null)[] = [];
    const editing = commitEdit(api, repo, LANTERN, (t) => {
      seen.push(t);
      if (seen.length === 1) mock.editOnGitHub(LANTERN, covered);
      return rename(t);
    });
    // Whatever reads the file next waits out the retry, not just the first try.
    await settled(LANTERN);
    expect(mock.file(LANTERN)?.text).toBe(rename(covered));
    await editing;
    expect(seen).toEqual([seeded, covered]);
    expect(mock.file(LANTERN)?.text).toBe(rename(covered));
    expect(ours(mock)).toHaveLength(1);
  });

  it("gives up with the conflict after three attempts, committing nothing", async () => {
    const { api, repo, mock } = mockConnection({ files: seedDecks() });
    let n = 0;
    const edit = commitEdit(api, repo, LANTERN, (t) => {
      n++;
      mock.editOnGitHub(LANTERN, `${t}# moved ${n}\n`);
      return rename(t);
    });
    await expect(edit).rejects.toBeInstanceOf(ConflictError);
    expect(n).toBe(3);
    expect(ours(mock)).toEqual([]);
  });

  it("writes nothing for an edit that changes nothing, or that throws", async () => {
    const { api, repo, mock } = mockConnection({ files: seedDecks() });
    const file = mock.file(LANTERN);
    expect(await commitEdit(api, repo, LANTERN, (t) => t)).toEqual({
      kind: "unchanged",
      ...file,
    });
    expect(await commitEdit(api, repo, LANTERN, () => null)).toMatchObject({
      kind: "unchanged",
    });
    await expect(
      commitEdit(api, repo, LANTERN, () => {
        throw new Error("refused");
      }),
    ).rejects.toThrow("refused");
    expect(puts(mock)).toEqual([]);
  });

  it("creates a file the repo lacks, and is made again on one created meanwhile", async () => {
    const decks = '\n[decks]\norder = ["decks/loam.deck.toml"]\n';
    const order = (t: string | null) => `${t ?? ""}${decks}`;
    const absent = mockConnection({ files: seedDecks() });
    await commitEdit(absent.api, absent.repo, SETTINGS_PATH, order);
    expect(absent.mock.file(SETTINGS_PATH)?.text).toBe(decks);

    const raced = mockConnection({ files: seedDecks() });
    const rules = '[printings]\nrank = [{ avoid = "is:digital" }]\n';
    const seen: (string | null)[] = [];
    await commitEdit(raced.api, raced.repo, SETTINGS_PATH, (t) => {
      seen.push(t);
      if (seen.length === 1) raced.mock.editOnGitHub(SETTINGS_PATH, rules);
      return order(t);
    });
    expect(seen).toEqual([null, rules]);
    expect(raced.mock.file(SETTINGS_PATH)?.text).toBe(order(rules));
  });

  it("asked for twice at once, makes the second on the first without a conflict", async () => {
    const { api, repo, mock } = mockConnection({ files: seedDecks() });
    const cover = (t: string | null) =>
      setDeckCover(t ?? "", { set: "cmr", num: "304" });
    await Promise.all([
      commitEdit(api, repo, LANTERN, rename),
      commitEdit(api, repo, LANTERN, cover),
    ]);
    expect(puts(mock)).toHaveLength(2);
    expect(mock.file(LANTERN)?.text).toBe(
      cover(rename(seedDecks()[LANTERN] ?? "")),
    );
  });
});
