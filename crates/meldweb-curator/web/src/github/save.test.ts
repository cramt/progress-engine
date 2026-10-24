import {
  afterEach,
  beforeAll,
  beforeEach,
  describe,
  expect,
  it,
  vi,
} from "vitest";
import { commitMessage } from "../deck";
import type { Conflict, Settle } from "../deck.gen";
import { settled, spotKey } from "./repoFile";
import {
  createSaveStore,
  IDLE_MS,
  leave,
  type PageEvents,
  type SaveStore,
} from "./save";
import { fileOf, loadWasm, messages, watchedRepo } from "./testkit";

// The repo, its merges and its messages are the wasm's.
beforeAll(loadWasm);

const PATH = "lantern.deck.toml";
const BRANCH = "decks/lantern/main";
const deck = (...cards: string[]) =>
  `name = "Lantern"\ncards = [\n${cards.map((c) => `  { name = "${c}" },\n`).join("")}]\n`;
const ORIGINAL = deck("Sol Ring");
/** The nth edit: n Mind Stones beside the Sol Ring. */
const v = (n: number) => deck("Sol Ring", ...Array(n).fill("Mind Stone"));
const ELSEWHERE = deck("Sol Ring", "Island");

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
let conn: ReturnType<typeof watchedRepo>;
const file = () => fileOf(conn.repo, PATH, BRANCH);
const log = () => messages(conn.repo, PATH, BRANCH);
const elsewhere = (text: string) => conn.elsewhere(PATH, text, BRANCH);

async function open(text: string, onMerge?: (text: string) => void) {
  conn = watchedRepo({ "decks/lantern.deck.toml": text });
  const read = await conn.repo.getFile(PATH, BRANCH);
  store = createSaveStore({
    repo: conn.repo,
    path: PATH,
    branch: BRANCH,
    text: read?.text ?? "",
    sha: read?.sha ?? null,
    ...(onMerge ? { onMerge } : {}),
  });
}

beforeEach(async () => {
  await open(ORIGINAL);
  vi.useFakeTimers();
});
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
    expect(conn.saves()).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(1);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(v(1));
    expect(await log()).toEqual([
      "add decks/lantern.deck.toml",
      commitMessage(ORIGINAL, v(1), PATH),
    ]);
  });

  it("restarts the idle clock on every edit, and commits the edits as one", async () => {
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(6000);
    store.edit(v(2));
    await vi.advanceTimersByTimeAsync(6000);
    expect(conn.saves()).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(4000);
    expect(conn.saves()).toHaveLength(1);
    expect(await file()).toBe(v(2));
  });

  it("commits nothing when undo returns to the saved text", async () => {
    store.edit(v(1));
    store.edit(ORIGINAL);
    expect(store.getState().status).toBe("saved");
    await vi.advanceTimersByTimeAsync(IDLE_MS * 2);
    expect(conn.saves()).toHaveLength(0);
  });

  it("an edit during a save is saved from that save's text, then on its own idle clock", async () => {
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    store.edit(v(2));
    const saving = store.flush();
    store.edit(v(3));
    await saving;
    expect(store.getState().status).toBe("unsaved");
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(v(3));
    // Each made from the one before, so none had to merge.
    expect(
      conn
        .saves()
        .map((s) => (s.made.kind === "from" ? s.made.base : s.made.kind)),
    ).toEqual([ORIGINAL, v(1), v(2)]);
  });

  it("commits at once when the page is hidden", async () => {
    const p = page();
    store.attach(p.events);
    store.edit(v(1));
    p.hide();
    await vi.advanceTimersByTimeAsync(0);
    expect(conn.saves()).toHaveLength(1);
    expect(await file()).toBe(v(1));
  });

  it("commits at once on pagehide (closing within the ten seconds)", async () => {
    const p = page();
    store.attach(p.events);
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(3000);
    p.pagehide();
    await vi.advanceTimersByTimeAsync(0);
    expect(conn.saves()).toHaveLength(1);
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
    expect(conn.saves()).toHaveLength(1);
  });

  it("reopening a deck waits for the save made while leaving it", async () => {
    store.edit(v(1));
    const leaving = leave(store);
    const reopen = settled(spotKey(PATH, BRANCH)).then(file);
    await leaving;
    expect(await reopen).toBe(v(1));
  });

  it("leaving while a save is landing waits for the edits made since, too", async () => {
    store.edit(v(1));
    const first = store.flush();
    store.edit(v(2));
    const leaving = leave(store);
    const reopen = settled(spotKey(PATH, BRANCH)).then(file);
    await first;
    await leaving;
    expect(await reopen).toBe(v(2));
  });

  it("a detached store no longer listens to the page", async () => {
    const p = page();
    const detach = store.attach(p.events);
    detach();
    store.edit(v(1));
    p.pagehide();
    await vi.advanceTimersByTimeAsync(0);
    expect(conn.saves()).toHaveLength(0);
  });
});

describe("a conflict, for an editor that takes no merges", () => {
  async function conflicted() {
    await elsewhere(ELSEWHERE);
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState()).toEqual({ status: "conflict", conflict: null });
    expect(conn.saves()).toHaveLength(1);
  }

  it("stops saving: further edits and hides commit nothing", async () => {
    await conflicted();
    const p = page();
    store.attach(p.events);
    store.edit(v(2));
    p.pagehide();
    await vi.advanceTimersByTimeAsync(IDLE_MS * 3);
    expect(conn.saves()).toHaveLength(1);
    expect(store.getState().status).toBe("conflict");
    expect(p.beforeunload()).toBe(true);
    expect(await file()).toBe(ELSEWHERE);
  });

  it("reload drops the local edits and takes the repo's file", async () => {
    await conflicted();
    expect(await store.reload()).toBe(ELSEWHERE);
    expect(store.getState().status).toBe("saved");
    // Saving resumes from the repo's text.
    store.edit(v(5));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(v(5));
  });

  it("overwrite commits the local text over the repo's, said against it", async () => {
    await conflicted();
    store.edit(v(2));
    await store.overwrite();
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(v(2));
    expect((await log()).at(-1)).toBe(commitMessage(ELSEWHERE, v(2), PATH));
  });

  it("a save asked for during an overwrite waits for the overwrite", async () => {
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
    expect(await file()).toBe(v(3));
    // The refused save, the overwrite, then the hide's save, from the overwrite's text.
    expect(conn.saves().map((s) => s.made)).toEqual([
      { kind: "from", base: ORIGINAL, moved: { kind: "refuse" } },
      { kind: "over" },
      { kind: "from", base: v(2), moved: { kind: "refuse" } },
    ]);
  });

  it("an overwrite asked for during a save runs after it", async () => {
    store.edit(v(1));
    const saving = store.flush();
    const other = elsewhere(ELSEWHERE);
    const overwriting = store.overwrite();
    await Promise.all([saving, other, overwriting]);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(v(1));
  });
});

describe("a save another client got in ahead of", () => {
  let merged: string[];

  beforeEach(async () => {
    vi.useRealTimers();
    store.dispose();
    merged = [];
    await open(deck("a", "b", "c", "d"), (text) => merged.push(text));
    vi.useFakeTimers();
  });

  const conflictOf = (): Conflict => {
    const state = store.getState();
    if (state.status !== "conflict" || !state.conflict)
      throw new Error(`no conflict: ${JSON.stringify(state)}`);
    return state.conflict;
  };
  const clashesOf = () =>
    conflictOf().files.flatMap((f) =>
      f.needs === "eachClash"
        ? f.clashes.map((c) => [c.subject, c.ours, c.theirs])
        : [],
    );
  const picking = (...picks: ("ours" | "theirs")[]): Settle => ({
    guard: conflictOf().guard,
    files: [{ needs: "eachClash", path: PATH, picks }],
  });

  it("merges the repo's edits with its own in one save, commits them, and hands them back", async () => {
    await elsewhere(deck("a", "b", "c", "D"));
    store.edit(deck("A", "b", "c", "d"));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(deck("A", "b", "c", "D"));
    expect(merged).toEqual([deck("A", "b", "c", "D")]);
    expect(conn.saves()).toHaveLength(1);
    // The commit says what this page changed, over the file as it was.
    expect((await log()).at(-1)).toBe(
      commitMessage(deck("a", "b", "c", "D"), deck("A", "b", "c", "D"), PATH),
    );
    // Saving goes on from the merge.
    store.edit(deck("A", "b", "c", "D", "e"));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(deck("A", "b", "c", "D", "e"));
  });

  it("leaves edits made while it merged to the next save, which merges them too", async () => {
    await elsewhere(deck("a", "b", "c", "D"));
    store.edit(deck("A", "b", "c", "d"));
    const saving = store.flush();
    store.edit(deck("A", "B", "c", "d"));
    await saving;
    // The editor's newer text is not overwritten by the merge.
    expect(merged).toEqual([]);
    expect(store.getState().status).toBe("unsaved");
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(deck("A", "B", "c", "D"));
    expect(merged).toEqual([deck("A", "B", "c", "D")]);
  });

  it("is a conflict naming the card when both changed it differently", async () => {
    const two = (n: number) =>
      deck("a", "b", "c", "d").replace('"b" }', `"b", qty = ${n} }`);
    await elsewhere(two(3));
    store.edit(two(2));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(conflictOf().files).toEqual([
      {
        needs: "eachClash",
        path: PATH,
        clashes: [
          {
            index: 0,
            subject: "b",
            attribute: "quantity",
            ours: "2",
            theirs: "3",
          },
        ],
      },
    ]);
    expect(merged).toEqual([]);
    expect(await file()).toBe(two(3));
  });

  it("settles each clash by the side picked, and commits the merge over the other save", async () => {
    const both = (b: number, d: number) =>
      deck("a", "b", "c", "d")
        .replace('"b" }', `"b", qty = ${b} }`)
        .replace('"d" }', `"d", qty = ${d} }`);
    // Elsewhere: b 3, d 5, and a new card; here: b 2, d 4.
    const withE = (text: string) =>
      text.replace('  { name = "d"', '  { name = "e" },\n  { name = "d"');
    await elsewhere(withE(both(3, 5)));
    store.edit(both(2, 4));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(clashesOf()).toEqual([
      ["b", "2", "3"],
      ["d", "4", "5"],
    ]);
    const before = (await log()).length;

    // Keep b as here, take d from there.
    await store.resolve(picking("ours", "theirs"));
    const want = withE(both(2, 5));
    expect(merged).toEqual([want]);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(want);
    // One commit, over the other save, saying what this page changed.
    expect(await log()).toHaveLength(before + 1);
    expect((await log()).at(-1)).toBe(
      commitMessage(withE(both(3, 5)), want, PATH),
    );
  });

  it("picks made before the file moved again settle nothing, and the clashes are shown anew", async () => {
    const two = (n: number) =>
      deck("a", "b", "c", "d").replace('"b" }', `"b", qty = ${n} }`);
    await elsewhere(two(3));
    store.edit(two(2));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    const stale = picking("theirs");
    await elsewhere(two(7));
    await store.resolve(stale);
    expect(clashesOf()).toEqual([["b", "2", "7"]]);
    expect(merged).toEqual([]);
    expect(await file()).toBe(two(7));
    // Picked against what is there now, they settle it.
    await store.resolve(picking("theirs"));
    expect(store.getState().status).toBe("saved");
    expect(merged).toEqual([two(7)]);
  });

  it("is a conflict with nothing to pick when the repo's file is no longer a deck", async () => {
    const now = await conn.repo.getFile(PATH, BRANCH);
    await conn.repo.putFile(PATH, {
      text: "cards = [\n",
      message: "broken elsewhere",
      sha: now?.sha ?? null,
      branch: BRANCH,
    });
    store.edit(deck("A", "b", "c", "d"));
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(conflictOf().files).toEqual([
      {
        needs: "wholeFile",
        path: PATH,
        why: { kind: "unreadable", reason: expect.any(String) },
      },
    ]);
    expect(clashesOf()).toEqual([]);
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
    expect(await file()).toBe(v(2));
    expect(store.getState().status).toBe("saved");
  });

  it("settles at once with nothing to save", async () => {
    expect(await store.settle()).toBe(true);
    expect(conn.saves()).toHaveLength(0);
  });

  it("does not settle in a conflict: the edits cannot be committed", async () => {
    await elsewhere(ELSEWHERE);
    store.edit(v(1));
    expect(await store.settle()).toBe(false);
    expect(store.getState().status).toBe("conflict");
  });

  it("does not settle when saving fails, and the page warns before unload", async () => {
    const p = page();
    store.attach(p.events);
    conn.failWrites(true);
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
    conn.failWrites(true);
    store.edit(v(1));
    await vi.advanceTimersByTimeAsync(IDLE_MS + 10);
    expect(store.getState()).toMatchObject({ status: "error" });
    conn.failWrites(false);
    await vi.advanceTimersByTimeAsync(IDLE_MS + 10);
    expect(store.getState().status).toBe("saved");
    expect(await file()).toBe(v(1));
  });

  it("is refused, not retried as a conflict, when the text is no deck", async () => {
    store.edit("cards = [\n");
    await vi.advanceTimersByTimeAsync(IDLE_MS);
    expect(store.getState()).toMatchObject({ status: "error" });
    expect(await file()).toBe(ORIGINAL);
  });
});
