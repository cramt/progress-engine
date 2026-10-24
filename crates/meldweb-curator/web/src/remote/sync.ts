/**
 * Keeping the browser's repo and its remote in step (ADR-0041). A sync is:
 * fetch, bring each of the remote's branches into the branch of its name
 * here (`main`, and each deck's `decks/<stem>/<branch>`), push what the
 * remote lacks: branches and snapshot tags made, moved or deleted here.
 *
 * - It runs when the repo opens, when the page comes back into view, and a
 *   few seconds after the repo gains a commit or a snapshot to push.
 * - Bringing in is Rust's, branch by branch: a fast-forward when only the
 *   remote moved, the remote's history when this repo is fresh, a branch
 *   only the remote has made here, otherwise a merge commit with two
 *   parents, merging each changed file by lines from the merge base. A
 *   remote still at repo version 1 is migrated here first, as it would be
 *   where it lives, and pushed there migrated, `main` as a fast-forward.
 * - A push names the remote ref's id as last advertised, so the host refuses
 *   it if someone pushed meanwhile; then it fetches, merges and pushes again.
 * - A file both sides changed so that it does not merge (a deck's card both
 *   changed differently, another file's same lines), or remote history
 *   unrelated to this repo's, stops syncing until the user chooses, as a
 *   refused save stops saving (ADR-0021). The user picks a side for each of
 *   a deck's clashes, file by file and branch by branch, and the next sync
 *   makes the merge commit with those picks; picks made against tips that
 *   have moved since settle nothing, and the clashes are shown again.
 *
 * Tabs sync one at a time under a Web Lock; each step is atomic in the
 * repo's worker, so a save landing between steps is merged or pushed next.
 */
import type {
  RemoteFacts,
  Settle,
  Side,
  SyncConflict,
  Synced,
  SyncSettle,
} from "../deck.gen";
import type { RemoteSteps } from "../repo/local";
import { type GitTransport, NeedsLogin } from "./transport";

/** How long after the last local commit a push waits, as saving waits. */
export const PUSH_IDLE_MS = 3_000;
/** A page coming back into view fetches at most this often. */
export const FOCUS_MS = 15_000;
/** How many times a refused push is merged and tried again. */
const PUSHES = 3;

const LOCK = "meldweb-sync";
const LAST_SYNC = "meldweb.lastSync";

export type Phase =
  | { kind: "idle" }
  | { kind: "syncing" }
  /** The host wants a login before it answers. */
  | { kind: "logged-out" }
  /**
   * These files changed here and on the remote in a way that does not
   * merge, each on its branch (`decks/lantern/main`, `lantern.deck.toml`),
   * with a deck's clashes card by card.
   */
  | { kind: "conflict"; conflicts: SyncConflict[] }
  /** The remote's history shares nothing with this repo's. */
  | { kind: "unrelated" }
  | { kind: "failed"; message: string };

export type SyncState =
  | { kind: "off" }
  /**
   * The repo could not say what its remote is or where it stands; every
   * sync asked for tries reading it again.
   */
  | { kind: "unreadable"; message: string }
  | {
      kind: "on";
      url: string;
      facts: RemoteFacts;
      /** Milliseconds since the epoch of the last sync that finished. */
      lastSynced: number | null;
      phase: Phase;
    };

export interface SyncOptions {
  steps: RemoteSteps;
  /** The transport for a remote's URL, carrying its host's login. */
  transport: (url: string) => GitTransport;
  /** Defaults to `navigator.locks`, or none. */
  lock?: <T>(work: () => Promise<T>) => Promise<T>;
  storage?: Pick<Storage, "getItem" | "setItem" | "removeItem"> | null;
  now?: () => number;
}

export interface SyncEngine {
  getState(): SyncState;
  subscribe(listener: () => void): () => void;
  /** Re-reads the remote's facts from the repo, without the network. */
  refresh(): Promise<void>;
  /** Syncs now, even when it stopped for the user to choose: "try again". */
  sync(): Promise<void>;
  /** After a conflict: takes `side` for everything that clashes, then syncs. */
  resolve(side: Side): Promise<void>;
  /**
   * After a conflict: settles each branch's merge by its own picks, each
   * under the guard of the conflict they were made against, then syncs.
   */
  settle(settles: readonly Settle[]): Promise<void>;
  /** After unrelated history: this repo becomes the remote's, then syncs. */
  replaceWithRemote(): Promise<void>;
  /** Makes `url` the remote and syncs with it. */
  connect(url: string): Promise<void>;
  /** Forgets the remote. */
  disconnect(): Promise<void>;
  /** The repo moved a commit or snapshot; a push follows once it is idle. */
  committed(): void;
  /** The page came back into view. */
  focused(): void;
  /** Main moved because of a sync: what pages show may be stale. */
  onMoved(listener: () => void): () => void;
}

const messageOf = (e: unknown) => (e instanceof Error ? e.message : String(e));

function defaultLock<T>(work: () => Promise<T>): Promise<T> {
  const locks = (globalThis.navigator as Navigator | undefined)?.locks;
  return locks ? (locks.request(LOCK, work) as Promise<T>) : work();
}

const defaultStorage = () =>
  typeof localStorage === "undefined" ? null : localStorage;

export function createSyncEngine(options: SyncOptions): SyncEngine {
  const { steps, transport } = options;
  const lock = options.lock ?? defaultLock;
  const storage =
    options.storage === undefined ? defaultStorage() : options.storage;
  const now = options.now ?? Date.now;

  let state: SyncState = { kind: "off" };
  const listeners = new Set<() => void>();
  const moved = new Set<() => void>();
  let running: Promise<void> | null = null;
  let again = false;
  let pushTimer: ReturnType<typeof setTimeout> | undefined;
  let lastFocus = 0;

  const set = (next: SyncState) => {
    state = next;
    for (const l of listeners) l();
  };

  function lastSynced(url: string): number | null {
    try {
      const raw = storage?.getItem(LAST_SYNC);
      const kept = raw
        ? (JSON.parse(raw) as { url?: unknown; at?: unknown })
        : null;
      return kept?.url === url && typeof kept.at === "number" ? kept.at : null;
    } catch {
      return null;
    }
  }

  async function read(phase?: Phase): Promise<SyncState> {
    let facts: RemoteFacts;
    try {
      facts = await steps.facts();
    } catch (e) {
      return { kind: "unreadable", message: messageOf(e) };
    }
    if (!facts.url) return { kind: "off" };
    const was =
      state.kind === "on" && state.url === facts.url ? state.phase : null;
    return {
      kind: "on",
      url: facts.url,
      facts,
      lastSynced: lastSynced(facts.url),
      phase: phase ?? was ?? { kind: "idle" },
    };
  }

  const stopped = () =>
    state.kind === "on" &&
    (state.phase.kind === "conflict" || state.phase.kind === "unrelated");

  type Stop =
    | { kind: "conflict"; conflicts: SyncConflict[] }
    | { kind: "unrelated" };

  /** Fetches and integrates; a stop when the user has to choose. */
  async function bringIn(
    url: string,
    git: GitTransport,
    settle?: SyncSettle,
  ): Promise<Stop | null> {
    const advertised = await git.advertise(url, "git-upload-pack");
    const want = await steps.fetchRequest(advertised);
    const response = want ? await git.post(url, "git-upload-pack", want) : null;
    await steps.fetchFinish(advertised, response);
    const synced: Synced = await steps.integrate(settle);
    if (
      synced.kind === "fastForwarded" ||
      synced.kind === "adopted" ||
      synced.kind === "merged"
    ) {
      for (const l of moved) l();
    }
    if (synced.kind === "conflict")
      return { kind: "conflict", conflicts: synced.conflicts };
    if (synced.kind === "unrelated") return { kind: "unrelated" };
    return null;
  }

  /** Pushes until the remote takes it, merging what it refused over. */
  async function pushOut(url: string, git: GitTransport): Promise<Stop | null> {
    for (let attempt = 1; ; attempt++) {
      const advertised = await git.advertise(url, "git-receive-pack");
      const plan = await steps.pushRequest(advertised);
      if (plan.kind === "nothing") return null;
      if (plan.kind === "send") {
        const response = await git.post(url, "git-receive-pack", plan.body);
        const pushed = await steps.pushFinish(plan.body, response);
        if (pushed.rejected.length === 0) return null;
        if (attempt >= PUSHES) {
          const why = pushed.rejected
            .map((r) => `${r.ref}: ${r.reason}`)
            .join("; ");
          throw new Error(`the remote refused the push (${why})`);
        }
      }
      // Behind, or refused because the remote moved since its refs were
      // read: take in what it has and push again.
      const stop = await bringIn(url, git);
      if (stop) return stop;
    }
  }

  async function once(settle?: SyncSettle, before?: () => Promise<void>) {
    await lock(async () => {
      await before?.();
      const current = await read();
      if (current.kind !== "on") {
        set(current);
        return;
      }
      set({ ...current, phase: { kind: "syncing" } });
      const { url } = current;
      let phase: Phase;
      try {
        const git = transport(url);
        const stop =
          (await bringIn(url, git, settle)) ?? (await pushOut(url, git));
        if (stop) phase = stop;
        else {
          storage?.setItem(LAST_SYNC, JSON.stringify({ url, at: now() }));
          phase = { kind: "idle" };
        }
      } catch (e) {
        phase =
          e instanceof NeedsLogin
            ? { kind: "logged-out" }
            : { kind: "failed", message: messageOf(e) };
      }
      set(await read(phase));
    });
  }

  /** One sync at a time; one asked for meanwhile runs after it. */
  function run(
    settle?: SyncSettle,
    before?: () => Promise<void>,
  ): Promise<void> {
    if (running) {
      if (settle === undefined && before === undefined) {
        again = true;
        return running;
      }
      return running.then(() => run(settle, before));
    }
    const go = once(settle, before).finally(() => {
      running = null;
      if (again) {
        again = false;
        if (!stopped()) void run();
      }
    });
    running = go;
    return go;
  }

  return {
    getState: () => state,
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    async refresh() {
      set(await read());
    },
    sync() {
      return run();
    },
    resolve(side) {
      return run({ prefer: side });
    },
    settle(settles) {
      return run({ settles: [...settles] });
    },
    replaceWithRemote() {
      return run(undefined, () => steps.adoptRemote());
    },
    connect(url) {
      return run(undefined, async () => {
        await steps.setRemote(url);
        storage?.removeItem(LAST_SYNC);
        // A new remote starts with nothing settled against it.
        if (state.kind === "on") set({ ...state, phase: { kind: "idle" } });
      });
    },
    async disconnect() {
      clearTimeout(pushTimer);
      await (running ?? Promise.resolve());
      await steps.removeRemote();
      storage?.removeItem(LAST_SYNC);
      set({ kind: "off" });
    },
    committed() {
      void (async () => {
        const next = await read();
        if (next.kind !== "on" || running) {
          if (!running) set(next);
          return;
        }
        if (state.kind === "on" && state.phase.kind === "syncing") return;
        set(next);
        const waiting =
          next.facts.ahead + next.facts.branches + next.facts.tags > 0;
        if (stopped() || !waiting) return;
        clearTimeout(pushTimer);
        pushTimer = setTimeout(() => void run(), PUSH_IDLE_MS);
      })().catch(() => {});
    },
    focused() {
      if (now() - lastFocus < FOCUS_MS || stopped()) return;
      lastFocus = now();
      void run();
    },
    onMoved(listener) {
      moved.add(listener);
      return () => moved.delete(listener);
    },
  };
}
