/**
 * Saving (ADR-0021, #120): edits collect in the page and are committed by
 * themselves once the deck has been idle ten seconds, or at once when the page
 * is hidden or closing. A save is one Contents `PUT` guarded by the blob sha
 * from the last load or save. When GitHub refuses it because the file moved,
 * saving stops until the user reloads or overwrites; nothing is merged.
 */
import { ConflictError, type GitHubApi, type RepoRef } from "./api";
import type { DeckText } from "./deckText";

export const IDLE_MS = 10_000;

export type SaveStatus = "unsaved" | "saving" | "saved" | "conflict" | "error";

export interface SaveState {
  status: SaveStatus;
  /** Why the last save failed, for `error`. */
  message?: string;
}

/**
 * The one way a deck file reaches GitHub, for an ordinary save, an overwrite
 * and a new deck alike. `sha: null` creates, and refuses an existing file.
 */
export function commitDeck(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  put: {
    text: string;
    message: string;
    sha: string | null;
    keepalive?: boolean;
  },
): Promise<{ sha: string }> {
  return api.putFile(repo, path, put);
}

export interface SaveStore {
  /** The deck file this store saves. */
  readonly path: string;
  getState(): SaveState;
  subscribe(listener: () => void): () => void;
  /** The deck's text after an edit (or an undo). Starts the idle clock. */
  edit(text: string): void;
  /** Saves now; `keepalive` lets the request outlive the page. */
  flush(options?: { keepalive?: boolean }): Promise<void>;
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
  api: GitHubApi;
  repo: RepoRef;
  path: string;
  /** The text as loaded from GitHub, and its blob sha. */
  text: string;
  sha: string | null;
  deckText: Pick<DeckText, "commitMessage">;
  idleMs?: number;
}

export function createSaveStore(options: SaveOptions): SaveStore {
  const { api, repo, path, deckText } = options;
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

  function save(keepalive: boolean): Promise<void> {
    clear();
    if (stopped()) return Promise.resolve();
    if (inflight) {
      // The sha comes back with the save in flight; the next one waits for it.
      if (keepalive) hideWhileSaving = true;
      return inflight;
    }
    if (!pending()) {
      if (state.status !== "saved") set({ status: "saved" });
      return Promise.resolve();
    }
    const before = base;
    const after = current;
    set({ status: "saving" });
    inflight = (async () => {
      try {
        const message = deckText.commitMessage(before, after, path);
        const r = await commitDeck(api, repo, path, {
          text: after,
          message,
          sha,
          keepalive: true,
        });
        base = after;
        sha = r.sha;
        set({ status: pending() ? "unsaved" : "saved" });
      } catch (e) {
        if (e instanceof ConflictError) set({ status: "conflict" });
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
    return inflight;
  }

  async function resolve(apply: () => Promise<void>) {
    clear();
    await inflight;
    await apply();
  }

  const store: SaveStore = {
    path,
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
    async reload() {
      let text = "";
      await resolve(async () => {
        const file = await api.getFile(repo, path);
        if (!file) throw new Error(`${path} is no longer on GitHub`);
        base = current = text = file.text;
        sha = file.sha;
        set({ status: "saved" });
      });
      return text;
    },
    async overwrite() {
      await resolve(async () => {
        set({ status: "saving" });
        try {
          const file = await api.getFile(repo, path);
          const after = current;
          const message = deckText.commitMessage(
            file?.text ?? base,
            after,
            path,
          );
          const r = await commitDeck(api, repo, path, {
            text: after,
            message,
            sha: file?.sha ?? null,
            keepalive: true,
          });
          base = after;
          sha = r.sha;
          set({ status: pending() ? "unsaved" : "saved" });
          schedule();
        } catch (e) {
          if (e instanceof ConflictError) set({ status: "conflict" });
          else
            set({
              status: "error",
              message: String((e as Error).message ?? e),
            });
        }
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

const leaving = new Map<string, Promise<void>>();

/**
 * Saves `store`'s pending edits as its editor goes away, remembered so that
 * opening the same deck again reads it only after that save has landed and
 * so starts from the new sha.
 */
export function flushOnLeave(store: SaveStore): Promise<void> {
  const done = store.flush().finally(() => {
    if (leaving.get(store.path) === done) leaving.delete(store.path);
  });
  leaving.set(store.path, done);
  return done;
}

/** Resolves once any save made while leaving `path` has finished. */
export function settled(path: string): Promise<void> {
  return leaving.get(path) ?? Promise.resolve();
}
