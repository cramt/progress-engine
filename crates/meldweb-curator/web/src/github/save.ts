/**
 * Saving (ADR-0021, #120): edits collect in the page and are committed by
 * themselves once the deck has been idle ten seconds, or at once when the page
 * is hidden or closing. A save is one write guarded by the blob sha from
 * the last load or save. When it is refused because the file moved, an
 * editor that takes merges has the repo's file and its own merged (a deck
 * card by card, anything else by lines) and committed; otherwise, or when
 * both changed the same thing differently, saving stops until the user
 * reloads or overwrites, and a deck's clashes say which cards.
 */
import type { MergeClash } from "../deck.gen";
import { ConflictError, type Repo } from "../repo/api";
import { mergeFile } from "./merge";
import { commitMessageFor, spotKey, track } from "./repoFile";

/** A refused save whose merge clashed, and on what. */
class Clashed extends ConflictError {
  constructor(
    message: string,
    readonly clashes: readonly MergeClash[],
  ) {
    super(message);
  }
}

/** How many times a merge is committed before a conflict is given up on. */
const MERGES = 3;

export const IDLE_MS = 10_000;

export type SaveState =
  | { status: "unsaved" | "saving" | "saved" }
  /** `clashes` are what in a deck both changed differently, when that is why. */
  | { status: "conflict"; clashes: readonly MergeClash[] }
  /** `message` is why the last save failed. */
  | { status: "error"; message: string };

export type SaveStatus = SaveState["status"];

export interface SaveStore {
  /** The file this store saves, as `spotKey` names it. */
  readonly key: string;
  getState(): SaveState;
  subscribe(listener: () => void): () => void;
  /** The deck's text after an edit (or an undo). Starts the idle clock. */
  edit(text: string): void;
  /** Saves now; `keepalive` lets the request outlive the page. */
  flush(options?: { keepalive?: boolean }): Promise<void>;
  /**
   * Saves until nothing is pending, for leaving the deck. `true` when every
   * edit is on GitHub; `false` when a conflict or a failed save holds some back.
   */
  settle(): Promise<boolean>;
  /** Conflict: drops local edits and takes GitHub's file. Returns its text. */
  reload(): Promise<string>;
  /** Conflict: re-reads the sha and commits the local text over GitHub's. */
  overwrite(): Promise<void>;
  /** Listens for hide, close and unload on the page; returns the detach. */
  attach(target?: PageEvents): () => void;
  /** Stops the idle clock and detaches. Later edits start it again. */
  dispose(): void;
}

/** The page events saving listens to, so tests can hand in their own. */
export interface PageEvents {
  window: Pick<Window, "addEventListener" | "removeEventListener">;
  document: Pick<
    Document,
    "addEventListener" | "removeEventListener" | "visibilityState"
  >;
}

export interface SaveOptions {
  repo: Repo;
  path: string;
  /** The branch the file is on: a deck's line. `main` when absent. */
  branch?: string;
  /** The text as loaded from GitHub, and its blob sha. */
  text: string;
  sha: string | null;
  idleMs?: number;
  /**
   * The editor's text became `text`, GitHub's changes merged into its own.
   * Without it a refused save is a conflict at once.
   */
  onMerge?: (text: string) => void;
}

export function createSaveStore(options: SaveOptions): SaveStore {
  const { repo, path, branch, onMerge } = options;
  const key = spotKey(path, branch);
  const on = branch === undefined ? {} : { branch };
  const idleMs = options.idleMs ?? IDLE_MS;

  let base = options.text;
  let sha = options.sha;
  let current = options.text;
  let state: SaveState = { status: "saved" };
  let timer: ReturnType<typeof setTimeout> | undefined;
  let inflight: Promise<void> | null = null;
  let hideWhileSaving = false;
  const listeners = new Set<() => void>();
  const detachers = new Set<() => void>();

  const set = (next: SaveState) => {
    state = next;
    for (const l of listeners) l();
  };
  const pending = () => current !== base;
  const stopped = () => state.status === "conflict";

  const clear = () => {
    if (timer !== undefined) clearTimeout(timer);
    timer = undefined;
  };
  const schedule = () => {
    clear();
    if (pending() && !stopped()) {
      timer = setTimeout(() => void save(false), idleMs);
    }
  };

  /**
   * The one commit in flight, which every later save, overwrite and reload
   * waits behind: each needs the sha this one comes back with.
   */
  function run(guard: () => Promise<{ before: string; sha: string | null }>) {
    set({ status: "saving" });
    // What is committed is the text as of now; later edits are the next save's.
    const after = current;
    const committing = (async () => {
      try {
        const from = await guard();
        try {
          await commit(from.before, from.sha, after);
        } catch (e) {
          if (!(e instanceof ConflictError) || !onMerge) throw e;
          await merge(onMerge, e);
        }
        set({ status: pending() ? "unsaved" : "saved" });
      } catch (e) {
        if (e instanceof ConflictError)
          set({
            status: "conflict",
            clashes: e instanceof Clashed ? e.clashes : [],
          });
        else
          set({ status: "error", message: String((e as Error).message ?? e) });
      }
    })().finally(() => {
      inflight = null;
      if (stopped()) return;
      if (hideWhileSaving && pending()) {
        hideWhileSaving = false;
        void save(true);
      } else {
        hideWhileSaving = false;
        // Edits made during the save, or a failed one, wait out the idle clock again.
        schedule();
      }
    });
    inflight = committing;
    void track(key, committing);
    return committing;
  }

  async function commit(before: string, guard: string | null, text: string) {
    const r = await repo.putFile(path, {
      text,
      message: commitMessageFor(path, before, text),
      sha: guard,
      keepalive: true,
      ...on,
    });
    base = text;
    sha = r.sha;
  }

  /**
   * Commits GitHub's file with the local edits merged in, on GitHub's sha.
   * Throws `refused` when the two changed the same lines, the file is gone,
   * or what the merge makes is not a file of its kind.
   */
  async function merge(onMerge: (text: string) => void, refused: Error) {
    for (let attempt = 1; ; attempt++) {
      const file = await repo.getFile(path, branch);
      if (!file) throw refused;
      // From the latest local text, not the one being committed: nothing
      // awaits between reading it and handing the merge back.
      const merging = mergeFile(path, base, current, file.text);
      if (merging.kind === "clashed")
        throw new Clashed(refused.message, merging.clashes);
      if (merging.kind !== "merged") throw refused;
      const merged = merging.text;
      // Rust refuses to diff a text that is not a file of its kind.
      let message: string;
      try {
        message = commitMessageFor(path, file.text, merged);
      } catch {
        throw refused;
      }
      current = merged;
      onMerge(merged);
      if (merged === file.text) {
        base = merged;
        sha = file.sha;
        return;
      }
      try {
        const r = await repo.putFile(path, {
          text: merged,
          message,
          sha: file.sha,
          keepalive: true,
          ...on,
        });
        base = merged;
        sha = r.sha;
        return;
      } catch (e) {
        if (!(e instanceof ConflictError) || attempt >= MERGES) throw e;
      }
    }
  }

  function save(keepalive: boolean): Promise<void> {
    clear();
    if (stopped()) return Promise.resolve();
    if (inflight) {
      if (keepalive) hideWhileSaving = true;
      return inflight;
    }
    if (!pending()) {
      if (state.status !== "saved") set({ status: "saved" });
      return Promise.resolve();
    }
    return run(async () => ({ before: base, sha }));
  }

  /** Waits out every commit in flight, including one a finished one started. */
  async function idle() {
    clear();
    while (inflight) await inflight;
  }

  const store: SaveStore = {
    key,
    getState: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    edit(text) {
      if (text === current) return;
      current = text;
      if (stopped()) return;
      if (!inflight) set({ status: pending() ? "unsaved" : "saved" });
      schedule();
    },
    flush(opts) {
      return save(opts?.keepalive ?? false);
    },
    async settle() {
      for (;;) {
        await idle();
        if (state.status === "conflict") return false;
        if (!pending()) return true;
        await save(false);
        if (state.status === "error") return false;
      }
    },
    async reload() {
      await idle();
      const file = await repo.getFile(path, branch);
      if (!file) throw new Error(`${path} is no longer in the repo`);
      base = current = file.text;
      sha = file.sha;
      set({ status: "saved" });
      return file.text;
    },
    async overwrite() {
      clear();
      // Claims `inflight` in this tick when it is free, so a save asked for
      // right after queues behind the overwrite rather than racing it.
      if (inflight) await idle();
      // Diffed against, and guarded by, what is on GitHub now.
      await run(async () => {
        const file = await repo.getFile(path, branch);
        return { before: file?.text ?? base, sha: file?.sha ?? null };
      });
    },
    attach(target) {
      const page: PageEvents = target ?? { window, document };
      const onVisibility = () => {
        if (page.document.visibilityState === "hidden") void save(true);
      };
      const onPageHide = () => void save(true);
      const onBeforeUnload = (e: BeforeUnloadEvent) => {
        if (state.status === "saved") return;
        e.preventDefault();
        // Older browsers show the prompt only when this is set.
        e.returnValue = "";
      };
      page.document.addEventListener("visibilitychange", onVisibility);
      page.window.addEventListener("pagehide", onPageHide);
      page.window.addEventListener("beforeunload", onBeforeUnload);
      const detach = () => {
        page.document.removeEventListener("visibilitychange", onVisibility);
        page.window.removeEventListener("pagehide", onPageHide);
        page.window.removeEventListener("beforeunload", onBeforeUnload);
        detachers.delete(detach);
      };
      detachers.add(detach);
      return detach;
    },
    dispose() {
      clear();
      for (const d of [...detachers]) d();
    },
  };
  return store;
}

/**
 * Saves every edit `store` still holds as its editor goes away, those made
 * while a save was landing too, tracked so that opening the same file again
 * reads it only after they have landed and so starts from the new sha.
 */
export function leave(store: SaveStore): Promise<boolean> {
  return track(store.key, store.settle());
}
