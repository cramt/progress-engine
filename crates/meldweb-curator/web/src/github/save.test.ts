import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import type { Repo } from "../repo/api";
import { settled } from "./repoFile";
import {
  createSaveStore,
  IDLE_MS,
  leave,
  type PageEvents,
  type SaveStore,
} from "./save";
import { fakeRepo, loadWasm } from "./testkit";

// The line merge is the wasm's.
beforeAll(loadWasm);

// `commitMessage` is the edits agent's wasm (#124); the save only needs the
// name, so it is stubbed through the same module the app imports.
vi.mock("../deck", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  commitMessage: (before: string, after: string, path: string) => {
    // Rust refuses to diff a text that is not a deck.
    if (after.includes("not a deck")) throw new Error("refused");
    return `${path}: ${before.length} -> ${after.length} bytes`;
  },
}));

const PATH = "lantern.deck.toml";
const ORIGINAL = 'name = "Lantern"\ncards = []\n';
const v = (n: number) => `name = "Lantern"\n# edit ${n}\ncards = []\n`;

function page() {
  const win = new EventTarget();
  const doc = Object.assign(new EventTarget(), {
    visibilityState: "visible" as DocumentVisibilityState,
  });
  return {
    events: { window: win, document: doc } as unknown as PageEvents,
    hide() {
      doc.visibilityState = "hidden";
      doc.dispatchEvent(new Event("visibilitychange"));
    },
    pagehide: () => win.dispatchEvent(new Event("pagehide")),
    /** Whether the browser would warn before leaving. */
    beforeunload() {
      const e = new Event("beforeunload", { cancelable: true });
      win.dispatchEvent(e);
      return e.defaultPrevented;
    },
  };
}

let store: SaveStore;
let conn: { repo: Repo; mock: ReturnType<typeof fakeRepo> };
const connect = (files: Record<string, string>) => {
  const mock = fakeRepo(files);
  return { repo: mock.repo, mock };
};

async function setup() {
  conn = connect({ [PATH]: ORIGINAL });
  const file = conn.mock.file(PATH);
  store = createSaveStore({
    repo: conn.repo,
    path: PATH,
    text: ORIGINAL,
    sha: file?.sha ?? null,
  });
  vi.useFakeTimers();
}

const puts = () => conn.mock.calls().filter((r) => r.method === "PUT");

beforeEach(setup);
afterEach(() => {
  store.dispose();
  vi.useRealTimers();
});

describe("saving by itself", () => {
  it("commits once the deck has been idle ten seconds, with the diff's message", async () => {
    expect(store.getState().status).toBe("saved");
    store.edit(v(1));
    expect(store.getState().status).toBe("unsaved");
    await vi.advanceTimersByTimeAsync(IDLE_MS - 1);
    expect(puts()).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(1));
    expect(conn.mock.commits().map((c) => c.message)).toEqual([
      `${PATH}: ${ORIGINAL.length} -> ${v(1).length} bytes`,
    ]);
  });

  it("restarts the idle clock on every edit, and commits the edits as one", async () => {
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(6000);
    store.edit(v(2));
    await vi.advanceTimersByTimeAsync(6000);
    expect(puts()).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(4000);
    expect(puts()).toHaveLength(1);
    expect(conn.mock.file(PATH)?.text).toBe(v(2));
  });

  it("commits nothing when undo returns to the saved text", async () => {
    store.edit(v(1));
    store.edit(ORIGINAL);
    expect(store.getState().status).toBe("saved");
    await vi.advanceTimersByTimeAsync(IDLE_MS * 2);
    expect(puts()).toHaveLength(0);
  });

  it("an edit during a save waits for that save's sha, then its own idle clock", async () => {
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    store.edit(v(2));
    const saving = store.flush();
    store.edit(v(3));
    await saving;
    expect(store.getState().status).toBe("unsaved");
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(3));
    expect(conn.mock.commits().every((c) => !c.outOfBand)).toBe(true);
  });

  it("commits at once, kept alive, when the page is hidden", async () => {
    const p = page();
    store.attach(p.events);
    store.edit(v(1));
    p.hide();
    await vi.advanceTimersByTimeAsync(0);
    expect(puts()).toEqual([expect.objectContaining({ keepalive: true })]);
    expect(conn.mock.file(PATH)?.text).toBe(v(1));
  });

  it("commits at once, kept alive, on pagehide (closing within the ten seconds)", async () => {
    const p = page();
    store.attach(p.events);
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(3000);
    p.pagehide();
    await vi.advanceTimersByTimeAsync(0);
    expect(puts()).toEqual([expect.objectContaining({ keepalive: true })]);
    expect(store.getState().status).toBe("saved");
  });

  it("warns before unload only while something is not saved", async () => {
    const p = page();
    store.attach(p.events);
    expect(p.beforeunload()).toBe(false);
    store.edit(v(1));
    expect(p.beforeunload()).toBe(true);
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(p.beforeunload()).toBe(false);
  });

  it("a second attach survives the first one's detach, as under StrictMode", async () => {
    const p = page();
    const first = store.attach(p.events);
    store.attach(p.events);
    first();
    await leave(store);
    store.edit(v(1));
    p.pagehide();
    await vi.advanceTimersByTimeAsync(0);
    expect(puts()).toHaveLength(1);
  });

  it("reopening a deck waits for the save made while leaving it", async () => {
    store.edit(v(1));
    const leaving = leave(store);
    let landed = false;
    const reopen = settled(PATH).then(() => {
      landed = conn.mock.file(PATH)?.text === v(1);
    });
    await leaving;
    await reopen;
    expect(landed).toBe(true);
  });

  it("leaving while a save is landing waits for the edits made since, too", async () => {
    store.edit(v(1));
    const first = store.flush();
    store.edit(v(2));
    const leaving = leave(store);
    let reopened: string | undefined;
    const reopen = settled(PATH).then(() => {
      reopened = conn.mock.file(PATH)?.text;
    });
    await first;
    await leaving;
    await reopen;
    expect(reopened).toBe(v(2));
  });

  it("a detached store no longer listens to the page", async () => {
    const p = page();
    const detach = store.attach(p.events);
    detach();
    store.edit(v(1));
    p.pagehide();
    await vi.advanceTimersByTimeAsync(0);
    expect(puts()).toHaveLength(0);
  });
});

describe("a conflict", () => {
  async function conflicted() {
    conn.mock.editElsewhere(PATH, "# edited on github.com\ncards = []\n");
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("conflict");
    expect(puts()).toHaveLength(1);
  }

  it("stops saving: further edits and hides commit nothing", async () => {
    await conflicted();
    const p = page();
    store.attach(p.events);
    store.edit(v(2));
    p.pagehide();
    await vi.advanceTimersByTimeAsync(IDLE_MS * 3);
    expect(puts()).toHaveLength(1);
    expect(store.getState().status).toBe("conflict");
    expect(p.beforeunload()).toBe(true);
  });

  it("reload drops the local edits and takes GitHub's file", async () => {
    await conflicted();
    const text = await store.reload();
    expect(text).toBe("# edited on github.com\ncards = []\n");
    expect(store.getState().status).toBe("saved");
    // Saving resumes from GitHub's sha.
    store.edit(v(5));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(5));
  });

  it("overwrite re-reads the sha and commits the local text over GitHub's", async () => {
    await conflicted();
    store.edit(v(2));
    await store.overwrite();
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(2));
    const last = conn.mock.commits().at(-1);
    expect(last?.outOfBand).toBe(false);
    // The message diffs against what was on GitHub, which is what the commit changes.
    expect(last?.message).toBe(
      `${PATH}: ${"# edited on github.com\ncards = []\n".length} -> ${v(2).length} bytes`,
    );
  });

  it("a save asked for during an overwrite waits for the overwrite's sha", async () => {
    await conflicted();
    const p = page();
    store.attach(p.events);
    store.edit(v(2));
    const overwriting = store.overwrite();
    store.edit(v(3));
    // Hiding the page mid-overwrite, as leaving the deck would.
    p.hide();
    await overwriting;
    await vi.advanceTimersByTimeAsync(0);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(3));
    // The conflicted PUT, the overwrite, then the hide's save: none refused.
    expect(puts()).toHaveLength(3);
  });

  it("an overwrite asked for during a save runs after it", async () => {
    store.edit(v(1));
    const saving = store.flush();
    conn.mock.editElsewhere(PATH, "# edited on github.com\ncards = []\n");
    const overwriting = store.overwrite();
    await Promise.all([saving, overwriting]);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(1));
  });
});

describe("a save another client got in ahead of", () => {
  const deck = (...cards: string[]) =>
    `name = "Lantern"\ncards = [\n${cards.map((c) => `  { name = "${c}" },\n`).join("")}]\n`;
  let merged: string[];

  beforeEach(async () => {
    vi.useRealTimers();
    store.dispose();
    conn = connect({ [PATH]: deck("a", "b", "c", "d") });
    const file = conn.mock.file(PATH);
    merged = [];
    store = createSaveStore({
      repo: conn.repo,
      path: PATH,
      text: file?.text ?? "",
      sha: file?.sha ?? null,
      onMerge: (text) => merged.push(text),
    });
    vi.useFakeTimers();
  });

  it("merges GitHub's edits with its own, commits them, and hands them back", async () => {
    conn.mock.editElsewhere(PATH, deck("a", "b", "c", "D"));
    store.edit(deck("A", "b", "c", "d"));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(deck("A", "b", "c", "D"));
    expect(merged).toEqual([deck("A", "b", "c", "D")]);
    // The commit says what this client changed, over GitHub's file.
    expect(conn.mock.commits().at(-1)?.outOfBand).toBe(false);
    // Saving goes on from the merge's sha.
    store.edit(deck("A", "b", "c", "D", "e"));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(deck("A", "b", "c", "D", "e"));
  });

  it("merges the edits made while the refused save was in flight too", async () => {
    conn.mock.editElsewhere(PATH, deck("a", "b", "c", "D"));
    store.edit(deck("A", "b", "c", "d"));
    const saving = store.flush();
    store.edit(deck("A", "B", "c", "d"));
    await saving;
    expect(conn.mock.file(PATH)?.text).toBe(deck("A", "B", "c", "D"));
    expect(store.getState().status).toBe("saved");
  });

  it("is a conflict naming the card when both changed it differently", async () => {
    const two = (n: number) =>
      deck("a", "b", "c", "d").replace('"b" }', `"b", qty = ${n} }`);
    conn.mock.editElsewhere(PATH, two(3));
    store.edit(two(2));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState()).toEqual({
      status: "conflict",
      clashes: [
        {
          path: PATH,
          index: 0,
          subject: "b",
          attribute: "quantity",
          ours: "2",
          theirs: "3",
        },
      ],
    });
    expect(merged).toEqual([]);
    expect(conn.mock.file(PATH)?.text).toBe(two(3));
  });

  it("settles each clash by the side picked, and commits the merge on the other save's sha", async () => {
    const both = (b: number, d: number) =>
      deck("a", "b", "c", "d")
        .replace('"b" }', `"b", qty = ${b} }`)
        .replace('"d" }', `"d", qty = ${d} }`);
    // Elsewhere: b 3, d 5, and a new card; here: b 2, d 4.
    const elsewhere = both(3, 5).replace(
      '  { name = "d"',
      '  { name = "e" },\n  { name = "d"',
    );
    conn.mock.editElsewhere(PATH, elsewhere);
    store.edit(both(2, 4));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    const state = store.getState();
    expect(state.status).toBe("conflict");
    expect(
      state.status === "conflict" &&
        state.clashes.map((c) => [c.subject, c.ours, c.theirs]),
    ).toEqual([
      ["b", "2", "3"],
      ["d", "4", "5"],
    ]);
    const before = conn.mock.commits().length;

    // Keep b as here, take d from there.
    await store.resolve(["ours", "theirs"]);
    const want = both(2, 5).replace(
      '  { name = "d"',
      '  { name = "e" },\n  { name = "d"',
    );
    expect(merged).toEqual([want]);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(want);
    // One commit, over the other save's (the fake refuses any other sha),
    // saying what this page changed.
    expect(conn.mock.commits()).toHaveLength(before + 1);
    expect(conn.mock.commits().at(-1)?.outOfBand).toBe(false);
  });

  it("picks made before the file moved again settle nothing, and the clashes are shown anew", async () => {
    const two = (n: number) =>
      deck("a", "b", "c", "d").replace('"b" }', `"b", qty = ${n} }`);
    conn.mock.editElsewhere(PATH, two(3));
    store.edit(two(2));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("conflict");
    conn.mock.editElsewhere(PATH, two(7));
    await store.resolve(["theirs"]);
    const state = store.getState();
    expect(
      state.status === "conflict" &&
        state.clashes.map((c) => [c.subject, c.ours, c.theirs]),
    ).toEqual([["b", "2", "7"]]);
    expect(merged).toEqual([]);
    expect(conn.mock.file(PATH)?.text).toBe(two(7));
    // Picked against what is there now, they settle it.
    await store.resolve(["theirs"]);
    expect(store.getState().status).toBe("saved");
    expect(merged).toEqual([two(7)]);
  });

  it("is a conflict when the merge is not a deck", async () => {
    conn.mock.editElsewhere(PATH, deck("a", "b", "c", "d", "not a deck"));
    store.edit(deck("A", "b", "c", "d"));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("conflict");
    expect(merged).toEqual([]);
  });
});

describe("leaving the deck", () => {
  it("settles once every edit is committed, even ones made during a save", async () => {
    store.edit(v(1));
    const saving = store.flush();
    store.edit(v(2));
    expect(await store.settle()).toBe(true);
    await saving;
    expect(conn.mock.file(PATH)?.text).toBe(v(2));
    expect(store.getState().status).toBe("saved");
  });

  it("settles at once with nothing to save", async () => {
    expect(await store.settle()).toBe(true);
    expect(puts()).toHaveLength(0);
  });

  it("does not settle in a conflict: the edits cannot be committed", async () => {
    conn.mock.editElsewhere(PATH, "# edited on github.com\ncards = []\n");
    store.edit(v(1));
    expect(await store.settle()).toBe(false);
    expect(store.getState().status).toBe("conflict");
  });

  it("does not settle when saving fails, and the page warns before unload", async () => {
    const p = page();
    store.attach(p.events);
    conn.mock.failWrites(true);
    store.edit(v(1));
    const settling = store.settle();
    await vi.advanceTimersByTimeAsync(10);
    expect(await settling).toBe(false);
    expect(store.getState()).toMatchObject({ status: "error" });
    expect(p.beforeunload()).toBe(true);
  });
});

describe("a failed save", () => {
  it("shows the error and retries after the idle time", async () => {
    conn.mock.failWrites(true);
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(IDLE_MS + 10);
    expect(store.getState()).toMatchObject({ status: "error" });
    conn.mock.failWrites(false);
    await vi.advanceTimersByTimeAsync(IDLE_MS + 10);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(1));
  });
});
