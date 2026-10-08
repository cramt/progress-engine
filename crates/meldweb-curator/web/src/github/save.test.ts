import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createSaveStore,
  flushOnLeave,
  IDLE_MS,
  type PageEvents,
  type SaveStore,
  settled,
} from "./save";
import { mockConnection } from "./testkit";

// `commitMessage` is the edits agent's wasm (#124); the save only needs the
// name, so it is stubbed through the same module the app imports.
vi.mock("../deck", async (importOriginal) => ({
  ...(await importOriginal<object>()),
  commitMessage: (before: string, after: string, path: string) =>
    `${path}: ${before.length} -> ${after.length} bytes`,
}));

const PATH = "decks/lantern.deck.toml";
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
let conn: ReturnType<typeof mockConnection>;

async function setup() {
  conn = mockConnection({ files: { [PATH]: ORIGINAL } });
  // The token is fetched before the clock is faked, as the page has one by the time it edits.
  await conn.auth.token();
  const file = conn.mock.file(PATH);
  store = createSaveStore({
    api: conn.api,
    repo: conn.repo,
    path: PATH,
    text: ORIGINAL,
    sha: file?.sha ?? null,
  });
  vi.useFakeTimers();
}

const puts = () => conn.mock.requests().filter((r) => r.method === "PUT");

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
    await flushOnLeave(store);
    store.edit(v(1));
    p.pagehide();
    await vi.advanceTimersByTimeAsync(0);
    expect(puts()).toHaveLength(1);
  });

  it("reopening a deck waits for the save made while leaving it", async () => {
    store.edit(v(1));
    const leaving = flushOnLeave(store);
    let landed = false;
    const reopen = settled(PATH).then(() => {
      landed = conn.mock.file(PATH)?.text === v(1);
    });
    await leaving;
    await reopen;
    expect(landed).toBe(true);
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
    conn.mock.editOnGitHub(PATH, "# edited on github.com\ncards = []\n");
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
    conn.mock.editOnGitHub(PATH, "# edited on github.com\ncards = []\n");
    const overwriting = store.overwrite();
    await Promise.all([saving, overwriting]);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(1));
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
    conn.mock.editOnGitHub(PATH, "# edited on github.com\ncards = []\n");
    store.edit(v(1));
    expect(await store.settle()).toBe(false);
    expect(store.getState().status).toBe("conflict");
  });

  it("does not settle when saving fails, and the page warns before unload", async () => {
    const p = page();
    store.attach(p.events);
    await conn.auth.logout();
    store.edit(v(1));
    const settling = store.settle();
    // A logged-out token check waits one task for other tabs' news.
    await vi.advanceTimersByTimeAsync(10);
    expect(await settling).toBe(false);
    expect(store.getState()).toMatchObject({ status: "error" });
    expect(p.beforeunload()).toBe(true);
  });
});

describe("a failed save", () => {
  it("shows the error and retries after the idle time", async () => {
    await conn.auth.logout();
    store.edit(v(1));
    // A logged-out token check waits one task for other tabs' news.
    await vi.advanceTimersByTimeAsync(IDLE_MS + 10);
    expect(store.getState()).toMatchObject({ status: "error" });
    conn.mock.logIn();
    await vi.advanceTimersByTimeAsync(IDLE_MS + 10);
    expect(store.getState().status).toBe("saved");
    expect(conn.mock.file(PATH)?.text).toBe(v(1));
  });
});
