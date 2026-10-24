import { beforeAll, describe, expect, it } from "vitest";
import { setDeckCover, setDeckMeta } from "../deck";
import { ConflictError } from "../repo/api";
import { deckFile } from "./decks";
import { commitEdit, openFile, settled, spotKey } from "./repoFile";
import { createSaveStore } from "./save";
import { SETTINGS_PATH } from "./settings";
import { fileOf, loadWasm, messages, watchedRepo } from "./testkit";

beforeAll(loadWasm);

// Lantern on its own line (ADR-0041).
const L = deckFile("lantern");
// A few cards and no name, so a rename is the deck's first.
const LANTERN_TEXT = () =>
  'format = "commander"\ncards = [\n  { name = "Lantern of Insight" },\n  { name = "Sol Ring" },\n]\n';
const editLantern = (
  repo: Parameters<typeof commitEdit>[0],
  edit: Parameters<typeof commitEdit>[2],
) => commitEdit(repo, L.path, edit, L.branch);
const connect = () => {
  const mock = watchedRepo({ "decks/lantern.deck.toml": LANTERN_TEXT() });
  return { repo: mock.repo, mock };
};
const lantern = (repo: Parameters<typeof commitEdit>[0]) =>
  repo.getFile(L.path, L.branch);

describe("opening a file", () => {
  it("reads it only once the save in flight to it has landed", async () => {
    const { repo } = connect();
    const loaded = await lantern(repo);
    if (!loaded) throw new Error("seeded");
    const store = createSaveStore({ repo, ...L, ...loaded });
    const renamed = setDeckMeta(loaded.text, "Lantern Control");
    store.edit(renamed);
    const saving = store.flush();
    const opened = await openFile(repo, L.path, L.branch);
    await saving;
    store.dispose();
    expect(opened).toEqual(await lantern(repo));
    expect(opened?.text).toBe(renamed);
  });

  it("is null for a file the repo does not have", async () => {
    const { repo } = connect();
    expect(await openFile(repo, SETTINGS_PATH)).toBeNull();
  });
});

describe("a one-shot edit", () => {
  const rename = (t: string | null) => setDeckMeta(t ?? "", "Lantern Control");
  const log = (repo: Parameters<typeof commitEdit>[0]) =>
    messages(repo, L.path, L.branch);
  const SEEDED = ["add decks/lantern.deck.toml"];

  it("commits the edit of the file as it is, with its diff's message", async () => {
    const { repo } = connect();
    const made = await commitEdit(repo, L.path, rename, L.branch);
    expect(made).toEqual({ kind: "committed", ...(await lantern(repo)) });
    expect(await log(repo)).toEqual([
      ...SEEDED,
      'lantern: name: none → "Lantern Control"',
    ]);
  });

  it("is made again on what the repo has when the file moved under it, keeping the other change", async () => {
    const { repo, mock } = connect();
    const seeded = (await lantern(repo))?.text ?? "";
    const covered = setDeckCover(seeded, { set: "cmr", num: "304" });
    const seen: (string | null)[] = [];
    const editing = editLantern(repo, (t) => {
      seen.push(t);
      if (seen.length === 1) void mock.elsewhere(L.path, covered, L.branch);
      return rename(t);
    });
    // Whatever reads the file next waits out the retry, not just the first try.
    await settled(spotKey(L.path, L.branch));
    expect(await fileOf(repo, L.path, L.branch)).toBe(rename(covered));
    await editing;
    expect(seen).toEqual([seeded, covered]);
    // Its own save, refused, then made again on the other one.
    expect(mock.saves()).toHaveLength(2);
    expect(await log(repo)).toHaveLength(SEEDED.length + 2);
  });

  it("gives up with the conflict after three attempts, committing nothing of its own", async () => {
    const { repo, mock } = connect();
    let n = 0;
    const edit = editLantern(repo, (t) => {
      n++;
      void mock.elsewhere(
        L.path,
        setDeckCover(t ?? "", { set: "cmr", num: `${n}` }),
        L.branch,
      );
      return rename(t);
    });
    await expect(edit).rejects.toBeInstanceOf(ConflictError);
    expect(n).toBe(3);
    expect(await log(repo)).toHaveLength(SEEDED.length + 3);
    expect((await lantern(repo))?.text).not.toContain("Lantern Control");
  });

  it("writes nothing for an edit that changes nothing, or that throws", async () => {
    const { repo, mock } = connect();
    const file = await lantern(repo);
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
    expect(mock.saves()).toEqual([]);
  });

  it("creates a file the repo lacks, and is made again on one created meanwhile", async () => {
    const decks = '\n[decks]\norder = ["decks/loam.deck.toml"]\n';
    const order = (t: string | null) => `${t ?? ""}${decks}`;
    const absent = connect();
    await commitEdit(absent.repo, SETTINGS_PATH, order);
    expect(await fileOf(absent.repo, SETTINGS_PATH)).toBe(decks);

    const raced = connect();
    const rules = '[printings]\nrank = [{ avoid = "is:digital" }]\n';
    const seen: (string | null)[] = [];
    await commitEdit(raced.repo, SETTINGS_PATH, (t) => {
      seen.push(t);
      if (seen.length === 1) void raced.mock.elsewhere(SETTINGS_PATH, rules);
      return order(t);
    });
    expect(seen).toEqual([null, rules]);
    expect(await fileOf(raced.repo, SETTINGS_PATH)).toBe(order(rules));
  });

  it("asked for twice at once, makes the second on the first without a conflict", async () => {
    const { repo, mock } = connect();
    const cover = (t: string | null) =>
      setDeckCover(t ?? "", { set: "cmr", num: "304" });
    await Promise.all([
      commitEdit(repo, L.path, rename, L.branch),
      commitEdit(repo, L.path, cover, L.branch),
    ]);
    expect(mock.saves()).toHaveLength(2);
    expect(await fileOf(repo, L.path, L.branch)).toBe(
      cover(rename(LANTERN_TEXT())),
    );
  });
});
