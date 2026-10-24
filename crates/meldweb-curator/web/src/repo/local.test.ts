import { beforeAll, describe, expect, it } from "vitest";
import { openMagicRepo } from "../github/repo";
import { createSaveStore } from "../github/save";
import { loadWasm } from "../github/testkit";
import { ConflictError } from "./api";
import type { Author } from "./author";
import { memoryDisk } from "./disk";
import {
  createLocalCore,
  createLocalRepo,
  type LocalCoreOptions,
} from "./local";

beforeAll(loadWasm);

const ALEX: Author = { name: "Alex", email: "alex@example.com" };
const PATH = "lantern.deck.toml";
/** Lantern's own line (ADR-0041). */
const BRANCH = "decks/lantern/main";
const deck = (...cards: string[]) =>
  `name = "Lantern"\ncards = [\n${cards.map((c) => `  { name = "${c}" },\n`).join("")}]\n`;

/** One lock for every "tab", as navigator.locks is one per origin. */
function mutex(): LocalCoreOptions["lock"] {
  let held: Promise<unknown> = Promise.resolve();
  return (_mode, work) => {
    const run = held.then(work);
    held = run.catch(() => undefined);
    return run;
  };
}

/** Tabs over one storage, each with its own worker's mirror. */
function browser() {
  const disk = memoryDisk();
  const lock = mutex();
  const cores: ReturnType<typeof createLocalCore>[] = [];
  const tab = (author: Author = ALEX) => {
    const core = createLocalCore({
      disk,
      lock,
      announce: () => {
        for (const other of cores) if (other !== core) other.moved();
      },
    });
    cores.push(core);
    return createLocalRepo(
      (request) => core.handle(request),
      () => author,
    );
  };
  return { disk, tab };
}

describe("the browser's repo", () => {
  it("is made on the first visit, empty, and opens at version 2", async () => {
    const { disk, tab } = browser();
    const repo = tab();
    expect(await repo.decks()).toEqual([]);
    expect(await openMagicRepo(repo)).toEqual({ kind: "open", version: 2 });
    expect((await repo.getFile("VERSION"))?.text).toBe("2\n");
    // A bare git dir, in the slot `current` names.
    expect(new TextDecoder().decode(disk.files.get("current"))).toBe("r1\n");
    expect(new TextDecoder().decode(disk.files.get("r1/HEAD"))).toBe(
      "ref: refs/heads/main\n",
    );
    expect(disk.files.has("r1/refs/heads/main")).toBe(true);
    const [made] = await repo.history("VERSION");
    expect(made).toMatchObject({
      message: "curator: this repo is at version 2",
      author: "Alex",
    });
  });

  it("guards a write by the file's blob, as GitHub's Contents API does", async () => {
    const { tab } = browser();
    const repo = tab();
    const { sha } = await repo.putFile(PATH, {
      text: deck("A"),
      message: "lantern: made",
      sha: null,
    });
    await expect(
      repo.putFile(PATH, { text: deck("B"), message: "again", sha: null }),
    ).rejects.toBeInstanceOf(ConflictError);
    // Another file's commit moves the tip but not this file.
    await repo.putFile("VERSION", { text: "1\n", message: "v", sha: null });
    const next = await repo.putFile(PATH, {
      text: deck("B"),
      message: "lantern: B",
      sha,
    });
    await expect(
      repo.putFile(PATH, { text: deck("C"), message: "stale", sha }),
    ).rejects.toBeInstanceOf(ConflictError);
    await expect(
      repo.deleteFile(PATH, { message: "stale", sha }),
    ).rejects.toBeInstanceOf(ConflictError);
    await repo.deleteFile(PATH, { message: "lantern: deleted", sha: next.sha });
    expect(await repo.getFile(PATH)).toBeNull();
  });

  it("keeps a deck's past: history, a revision and the file at it, and snapshots", async () => {
    const { tab } = browser();
    const repo = tab();
    let sha: string | null = null;
    for (const c of ["A", "B", "C"]) {
      ({ sha } = await repo.putFile(PATH, {
        text: deck(c),
        message: `lantern: ${c}`,
        sha,
      }));
    }
    const past = await repo.history(PATH);
    expect(past.map((r) => r.message)).toEqual([
      "lantern: C",
      "lantern: B",
      "lantern: A",
    ]);
    expect(past.every((r) => !Number.isNaN(Date.parse(r.date)))).toBe(true);
    const first = past[2];
    if (!first) throw new Error("three commits");
    expect((await repo.getFile(PATH, first.commit))?.text).toBe(deck("A"));
    expect(await repo.revision(first.commit)).toEqual(first);
    expect(await repo.revision("0".repeat(40))).toBeNull();
    expect(await repo.history(PATH, { perPage: 1, page: 2 })).toEqual([
      past[1],
    ]);

    const taken = await repo.takeSnapshot({
      tag: "decks/lantern/fnm",
      label: "FNM 14 Sep",
      commit: first.commit,
    });
    expect(taken).toMatchObject({ label: "FNM 14 Sep", commit: first.commit });
    await expect(
      repo.takeSnapshot({
        tag: "decks/lantern/fnm",
        label: "again",
        commit: first.commit,
      }),
    ).rejects.toBeInstanceOf(ConflictError);
    expect(await repo.snapshots("decks/lantern/")).toEqual([taken]);
    await repo.dropSnapshot("decks/lantern/fnm");
    expect(await repo.snapshots("decks/lantern/")).toEqual([]);
  });

  it("refuses a stale sha from a second tab, and the second tab sees the first's save", async () => {
    const { tab } = browser();
    const one = tab();
    const two = tab();
    const { sha } = await one.putFile(PATH, {
      text: deck("A"),
      message: "made",
      sha: null,
    });
    const read = await two.getFile(PATH);
    expect(read?.sha).toBe(sha);
    await one.putFile(PATH, { text: deck("B"), message: "one", sha });
    await expect(
      two.putFile(PATH, { text: deck("C"), message: "two", sha }),
    ).rejects.toBeInstanceOf(ConflictError);
    expect((await two.getFile(PATH))?.text).toBe(deck("B"));
  });

  it("merges two tabs' saves of one deck by card, on the deck's own line", async () => {
    const { tab } = browser();
    const one = tab();
    const two = tab();
    const base = deck("a", "b", "c", "d");
    const { sha } = await one.putFile(PATH, {
      text: base,
      message: "made",
      sha: null,
      branch: BRANCH,
    });
    const merged: string[] = [];
    const stores = [one, two].map((repo, i) =>
      createSaveStore({
        repo,
        path: PATH,
        branch: BRANCH,
        text: base,
        sha,
        idleMs: 60_000,
        ...(i === 1 ? { onMerge: (t: string) => merged.push(t) } : {}),
      }),
    );
    const [first, second] = stores;
    if (!first || !second) throw new Error("two stores");
    first.edit(deck("a", "b", "c", "D"));
    await first.flush();
    second.edit(deck("A", "b", "c", "d"));
    await second.flush();
    expect(second.getState().status).toBe("saved");
    expect(merged).toEqual([deck("A", "b", "c", "D")]);
    expect((await one.getFile(PATH, BRANCH))?.text).toBe(
      deck("A", "b", "c", "D"),
    );
    expect((await one.getFile(PATH))?.text).toBeUndefined();
    for (const s of stores) s.dispose();
  });

  it("travels as a zip of the bare repo, and refuses a zip that is none", async () => {
    const { tab } = browser();
    const repo = tab();
    await openMagicRepo(repo);
    await repo.putFile(PATH, { text: deck("A"), message: "made", sha: null });
    const zip = await repo.exportZip();
    // A zip's local file header.
    expect([...zip.slice(0, 4)]).toEqual([0x50, 0x4b, 0x03, 0x04]);

    const elsewhere = browser();
    const other = elsewhere.tab();
    await other.putFile("decks/other.deck.toml", {
      text: deck("X"),
      message: "theirs",
      sha: null,
    });
    const watching = elsewhere.tab();
    expect(await watching.getFile("decks/other.deck.toml")).not.toBeNull();
    await other.importZip(zip);
    expect((await other.getFile(PATH))?.text).toBe(deck("A"));
    expect(await other.getFile("decks/other.deck.toml")).toBeNull();
    // Another tab follows the slot the import switched to.
    expect((await watching.getFile(PATH))?.text).toBe(deck("A"));
    expect(new TextDecoder().decode(elsewhere.disk.files.get("current"))).toBe(
      "r2\n",
    );
    expect(
      [...elsewhere.disk.files.keys()].some((k) => k.startsWith("r1/")),
    ).toBe(false);

    await expect(other.importZip(new Uint8Array([1, 2, 3]))).rejects.toThrow(
      /not a zip/,
    );
    expect((await other.getFile(PATH))?.text).toBe(deck("A"));
  });

  it("signs commits with this browser's author", async () => {
    const { tab } = browser();
    const repo = tab({ name: "Curator", email: "curator@localhost" });
    await repo.putFile("VERSION", { text: "1\n", message: "v", sha: null });
    expect((await repo.history("VERSION"))[0]?.author).toBe("Curator");
  });

  it("blames each card on who last changed it, edits not saved on top", async () => {
    const { tab } = browser();
    const alex = tab();
    const bo = tab({ name: "Bo", email: "bo@example.com" });
    const made = await alex.putFile(PATH, {
      text: deck("Sol Ring", "Mind Stone"),
      message: "lantern: +1 Sol Ring, +1 Mind Stone",
      sha: null,
      branch: BRANCH,
    });
    const swapped = await bo.putFile(PATH, {
      text: deck("Sol Ring", "Fellwar Stone"),
      message: "lantern: +1 Fellwar Stone, -1 Mind Stone",
      sha: made.sha,
      branch: BRANCH,
    });
    const now = await alex.blame("lantern", BRANCH);
    expect(now?.at).toBe(swapped.commit);
    expect(
      now?.cards.map((c) => [c.index, c.blame.kind, c.changes.join(", ")]),
    ).toEqual([
      [0, "added", "+1 Sol Ring"],
      [1, "added", "+1 Fellwar Stone"],
    ]);
    const [sol, fellwar] = now?.cards ?? [];
    expect(sol?.blame.kind === "added" && sol.blame.added.author).toBe("Alex");
    expect(fellwar?.blame.kind === "added" && fellwar.blame.added.author).toBe(
      "Bo",
    );

    // Unsaved: one more Sol Ring, and Opt.
    const editing = await alex.blame(
      "lantern",
      BRANCH,
      deck("Sol Ring", "Fellwar Stone", "Opt").replace(
        '{ name = "Sol Ring" }',
        '{ name = "Sol Ring", qty = 2 }',
      ),
    );
    expect(editing?.cards.map((c) => [c.blame.kind, c.changes])).toEqual([
      ["changedUnsaved", ["Sol Ring: 1 → 2"]],
      ["added", ["+1 Fellwar Stone"]],
      ["addedUnsaved", ["+1 Opt"]],
    ]);

    // At the first commit, Mind Stone was there.
    const then = await alex.blame("lantern", made.commit);
    expect(then?.cards.map((c) => c.changes)).toEqual([
      ["+1 Sol Ring"],
      ["+1 Mind Stone"],
    ]);
    expect(await alex.blame("loam", BRANCH)).toBeNull();
  });
});
