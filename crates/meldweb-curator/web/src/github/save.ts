/**
 * Saving (ADR-0021, #120): edits collect in the page and are committed by
 * themselves once the deck has been idle ten seconds, or at once when the page
 * is hidden or closing. A save is one call to the repo (`Repo.save`), which
 * commits the text when the file has not moved since the text it was made
 * from. When it has, an editor that takes merges gets the repo's file and
 * its own merged (a deck card by card, anything else by lines) and
 * committed in that same call; otherwise, or when both changed the same
 * thing differently, saving stops until the user reloads or overwrites,
 * and a deck's clashes say which cards. For a deck the user can instead
 * pick a side for each clash: the merge with those picks becomes the
 * editor's text as one undoable step, committed as a merge that never
 * clashed is.
 */
import type { Conflict, Made, Settle } from "../deck.gen";
import type { Repo } from "../repo/api";
import { spotKey, track } from "./repoFile";

/** A save that stopped: what its merge stopped on, when it merged. */
class Stopped extends Error {
  constructor(
    message: string,
    readonly conflict: Conflict | null,
  ) {
    super(message);
  }
}

export const IDLE_MS = 10_000;

export type SaveState =
  | { status: "unsaved" | "saving" | "saved" }
  /** `conflict` is what the save's merge stopped on, when it merged. */
  | { status: "conflict"; conflict: Conflict | null }
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
  /** Conflict: commits the local text over the repo's, whatever it is now. */
  overwrite(): Promise<void>;
  /**
   * Conflict: merges again settled by `settle`, picked against the
   * conflict, hands the merge to the editor and commits it. When the file
   * or the editor's text moved since the clashes were shown, the picks may
   * mean other cards, so they settle nothing and the conflict names the
   * clashes as they are now.
   */
  resolve(settle: Settle): Promise<void>;
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
  /** The text as loaded, and its blob sha: null while there is no file. */
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

  /** The text the repo's file and the editor last agreed on; null: no file. */
  let base: string | null = options.sha === null ? null : options.text;
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
  const pending = () => current !== (base ?? options.text);
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
   * waits behind: each starts from the text this one leaves. `work` gets
   * the text as of now; later edits are the next save's.
   */
  function run(work: (after: string) => Promise<void>) {
    set({ status: "saving" });
    const after = current;
    const committing = (async () => {
      try {
        await work(after);
        set({ status: pending() ? "unsaved" : "saved" });
      } catch (e) {
        if (e instanceof Stopped)
          set({ status: "conflict", conflict: e.conflict });
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

  /** Made from the text the repo and the editor last agreed on. */
  const fromBase = (settle?: Settle): Made => ({
    kind: "from",
    ...(base === null ? {} : { base }),
    moved: onMerge
      ? { kind: "merge", ...(settle ? { settle } : {}) }
      : { kind: "refuse" },
  });

  /** Saves `after` as `made` says, and takes in what the repo made of it. */
  async function commit(after: string, made: Made) {
    const saved = await repo.save({ path, ...on, text: after, made });
    switch (saved.kind) {
      case "committed":
      case "unchanged":
        if (saved.merged === undefined) {
          base = after;
        } else if (current === after) {
          base = current = saved.merged;
          onMerge?.(saved.merged);
        } else {
          // Edits made while this merged were made from `after`, so the
          // next save merges them into what the file is now.
          base = after;
        }
        return;
      case "clashed":
        throw new Stopped(`${path} clashed`, saved.conflict);
      case "moved":
        throw new Stopped(saved.message, null);
      case "refused":
        throw new Error(saved.message);
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
    return run((after) => commit(after, fromBase()));
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
      set({ status: "saved" });
      return file.text;
    },
    async overwrite() {
      clear();
      // Claims `inflight` in this tick when it is free, so a save asked for
      // right after queues behind the overwrite rather than racing it.
      if (inflight) await idle();
      // Over whatever the file is now, and said against it.
      await run((after) => commit(after, { kind: "over" }));
    },
    async resolve(settle) {
      clear();
      if (inflight) await idle();
      if (state.status !== "conflict" || !onMerge) return;
      await run((after) => commit(after, fromBase(settle)));
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
 * reads it only after they have landed and so starts from what they left.
 */
export function leave(store: SaveStore): Promise<boolean> {
  return track(store.key, store.settle());
}
