/**
 * The Magic repo in this browser (ADR-0041): a bare git repo of loose
 * objects in the origin's private file system, read and written by
 * `meldweb-git` in the wasm.
 *
 * `createLocalCore` runs in the repo's worker. `meldweb-git` reads a store
 * synchronously and OPFS is asynchronous, so the core loads the repo's files
 * into the wasm's mirror once, runs every operation there, and writes what
 * the operation's journal names through to OPFS, objects before refs, before
 * it answers.
 *
 * Tabs share the files but each holds its own mirror. A write holds the
 * repo's Web Lock and re-reads the refs inside it, loading whatever objects
 * another tab wrote, so the blob guard and the branch's compare-and-swap
 * see the repo as it is: two tabs saving one deck end in the same
 * `ConflictError` as two clients of GitHub's API. A read takes the lock
 * shared, and re-reads the refs only after another tab said it moved them.
 *
 * `createLocalRepo` is the page's side: a `Repo` over whatever carries
 * requests to the core, postMessage in the browser and a plain call in tests.
 */

import type {
  BranchDeleted,
  BranchFacts,
  BranchMade,
  CommitFacts,
  DeckBlame,
  DeckBranches,
  DeckDeleted,
  DeckLineList,
  Fetched,
  FileRead,
  FollowingRead,
  Forked,
  Journal,
  Listing,
  MergeDone,
  MergePlan,
  PushOutcome,
  RemoteFacts,
  RevisionRead,
  Revisions,
  Snapshots,
  Synced,
  TagFacts,
  Tagged,
  Who,
  Wrote,
} from "../deck.gen";
import { GitRepo } from "../wasm/pkg/meldweb_wasm.js";
import {
  type BlameRead,
  type Branch,
  ConflictError,
  type DeckLine,
  type DirEntry,
  type FileAt,
  type Following,
  type Repo,
  type Revision,
  type Snapshot,
  type TheirDeck,
} from "./api";
import type { Author } from "./author";
import type { Disk } from "./disk";

export type Request =
  | { kind: "getFile"; path: string; at?: string }
  | { kind: "getBytes"; path: string }
  | { kind: "listDir"; path: string }
  | {
      kind: "history";
      path: string;
      branch?: string;
      /** Seconds since the Unix epoch. */
      until?: number;
      page: number;
      perPage: number;
    }
  | { kind: "revision"; commit: string }
  | { kind: "blame"; stem: string; at: string; unsaved?: string }
  | { kind: "snapshots"; prefix: string }
  | {
      kind: "takeSnapshot";
      tag: string;
      label: string;
      commit: string;
      who: Who;
    }
  | { kind: "dropSnapshot"; tag: string }
  | {
      kind: "putFile";
      path: string;
      text: string;
      sha: string | null;
      message: string;
      who: Who;
      branch?: string;
      from?: string;
    }
  | { kind: "decks" }
  | { kind: "deleteDeck"; stem: string; commit: string }
  | { kind: "migrate"; from: number }
  | { kind: "branches"; stem: string }
  | { kind: "createBranch"; stem: string; name: string; at: string }
  | { kind: "deleteBranch"; stem: string; name: string; commit: string }
  | {
      kind: "planMerge";
      stem: string;
      from: string;
      into: string;
      prefer?: Side;
    }
  | {
      kind: "merge";
      stem: string;
      from: string;
      into: string;
      intoTip: string;
      fromTip: string;
      prefer?: Side;
      who: Who;
    }
  | {
      kind: "deleteFile";
      path: string;
      sha: string;
      message: string;
      who: Who;
    }
  | { kind: "exportZip" }
  | { kind: "importZip"; bytes: Uint8Array }
  | { kind: "remote" }
  | { kind: "setRemote"; url: string }
  | { kind: "removeRemote" }
  | { kind: "fetchRequest"; advertised: Uint8Array }
  | {
      kind: "fetchFinish";
      advertised: Uint8Array;
      response: Uint8Array | null;
    }
  | { kind: "integrate"; who: Who; prefer?: Side }
  | { kind: "adoptRemote" }
  | { kind: "followRequest"; theirs: TheirDeck; advertised: Uint8Array }
  | {
      kind: "followFinish";
      theirs: TheirDeck;
      advertised: Uint8Array;
      response: Uint8Array | null;
    }
  | { kind: "fork"; theirs: TheirDeck; stem: string; who: Who }
  | { kind: "following"; stem: string }
  | { kind: "unfollow"; stem: string; who: Who }
  | { kind: "planTake"; stem: string; prefer?: Side }
  | {
      kind: "takeUpstream";
      stem: string;
      intoTip: string;
      fromTip: string;
      prefer?: Side;
      who: Who;
    }
  | { kind: "pushRequest"; advertised: Uint8Array }
  | { kind: "pushFinish"; sent: Uint8Array; response: Uint8Array };

/** Which side a path both sides changed takes, once the user has chosen. */
export type Side = "ours" | "theirs";

/** What a push's ref advertisement came to; see `GitRepo.push_request`. */
export type PushPlan =
  | { kind: "nothing" }
  /** The remote's main has commits this repo has not merged. */
  | { kind: "behind" }
  | { kind: "send"; body: Uint8Array };

export interface Results {
  getFile: FileAt | null;
  getBytes: Uint8Array | null;
  listDir: DirEntry[];
  history: Revision[];
  revision: Revision | null;
  blame: BlameRead | null;
  snapshots: Snapshot[];
  takeSnapshot: Snapshot;
  dropSnapshot: null;
  putFile: { sha: string; commit: string };
  deleteFile: null;
  decks: DeckLine[];
  deleteDeck: null;
  migrate: null;
  branches: Branch[];
  createBranch: Branch;
  deleteBranch: null;
  planMerge: MergePlan;
  merge: MergeDone;
  exportZip: Uint8Array;
  importZip: null;
  remote: RemoteFacts;
  setRemote: null;
  removeRemote: null;
  fetchRequest: Uint8Array | null;
  fetchFinish: null;
  integrate: Synced;
  adoptRemote: null;
  pushRequest: PushPlan;
  pushFinish: PushOutcome;
  followRequest: Uint8Array | null;
  followFinish: Fetched;
  fork: Forked;
  following: Following | null;
  unfollow: null;
  planTake: MergePlan;
  takeUpstream: MergeDone;
}

export type Kind = Request["kind"];

/** What crosses back from the core: a result, or why there is none. */
export type Reply<K extends Kind = Kind> =
  | { kind: "ok"; value: Results[K] }
  /** Another tab, or this one, moved the file first: `ConflictError`. */
  | { kind: "conflict"; message: string }
  | { kind: "failed"; message: string };

export interface LocalCoreOptions {
  disk: Disk;
  /**
   * Runs `work` holding the repo's lock across every tab: `exclusive` for a
   * write, `shared` for re-reading the refs.
   */
  lock<T>(mode: "exclusive" | "shared", work: () => Promise<T>): Promise<T>;
  /** Tells every other tab that this one moved the repo. */
  announce(): void;
}

export interface LocalCore {
  handle<K extends Kind>(
    request: Extract<Request, { kind: K }>,
  ): Promise<Reply<K>>;
  /** Another tab moved the repo: re-read the refs before the next read. */
  moved(): void;
}

/** The file naming the slot that holds the repo, so an import swaps in whole. */
const POINTER = "current";
const FIRST_SLOT = "r1";
/** How many files are read from storage at once on opening. */
const READS = 64;

const iso = (seconds: number) => new Date(seconds * 1000).toISOString();
const text = (bytes: Uint8Array) => new TextDecoder().decode(bytes);

const revisionOf = (c: CommitFacts): Revision => ({
  commit: c.commit,
  message: c.message,
  date: iso(c.committedAt),
  author: c.author,
});

const branchOf = (b: BranchFacts): Branch => ({
  name: b.name,
  commit: b.commit,
  date: iso(b.committedAt),
  unmerged: b.unmerged,
  behind: b.behind,
});

const snapshotOf = (t: TagFacts): Snapshot => ({
  tag: t.tag,
  label: t.label,
  commit: t.commit,
  date: t.taggedAt === undefined ? "" : iso(t.taggedAt),
});

class Conflict extends Error {}

export function createLocalCore(options: LocalCoreOptions): LocalCore {
  const { disk, lock, announce } = options;
  let slot = FIRST_SLOT;
  /** The mirror, or null until loaded and after storage failed under it. */
  let git: GitRepo | null = null;
  /** Whether no other tab has said it moved the refs since they were read. */
  let fresh = false;
  // One operation at a time: each may reload the mirror under the others.
  let queue: Promise<unknown> = Promise.resolve();

  const at = (path: string) => `${slot}/${path}`;

  async function pointer(): Promise<string> {
    const bytes = await disk.read(POINTER);
    return bytes ? text(bytes).trim() : FIRST_SLOT;
  }

  async function readAll(
    paths: string[],
    into: (path: string, bytes: Uint8Array) => void,
  ) {
    for (let i = 0; i < paths.length; i += READS) {
      await Promise.all(
        paths.slice(i, i + READS).map(async (p) => {
          const bytes = await disk.read(at(p));
          if (bytes) into(p, bytes);
        }),
      );
    }
  }

  async function loadAll(): Promise<GitRepo> {
    const mirror = new GitRepo();
    const { files, dirs } = await disk.list(slot);
    for (const d of dirs) mirror.load_dir(d);
    await readAll(files, (p, bytes) => mirror.load(p, bytes));
    mirror.open();
    return mirror;
  }

  /** The mirror brought up to storage. The caller holds the lock. */
  async function sync(): Promise<GitRepo> {
    const named = await pointer();
    if (!git || named !== slot) {
      git?.free();
      git = null;
      slot = named;
      git = await loadAll();
      fresh = true;
      return git;
    }
    const mirror = git;
    const refs = (await disk.list(at("refs"))).files.map((p) => `refs/${p}`);
    const paths: string[] = [];
    const contents: string[] = [];
    await readAll(["HEAD", "packed-refs", ...refs], (p, bytes) => {
      paths.push(p);
      contents.push(text(bytes));
    });
    if (mirror.replace_refs(paths, contents)) {
      // Objects never change, so only the ones this tab lacks are read.
      const objects = (await disk.list(at("objects"))).files
        .map((p) => `objects/${p}`)
        .filter((p) => !mirror.has(p));
      await readAll(objects, (p, bytes) => mirror.load(p, bytes));
    }
    fresh = true;
    return mirror;
  }

  /** Writes the mirror's journal through to storage, in order. */
  async function flush(mirror: GitRepo, into = slot): Promise<void> {
    const journal = JSON.parse(mirror.take_journal()) as Journal;
    try {
      for (const step of journal.steps) {
        const path = `${into}/${step.path}`;
        if (step.kind === "dir") await disk.makeDir(path);
        else if (step.kind === "remove") await disk.remove(path);
        else {
          const bytes = mirror.bytes(step.path);
          if (!bytes) throw new Error(`the mirror lost ${step.path}`);
          await disk.write(path, bytes);
        }
      }
    } catch (e) {
      // The mirror is ahead of storage now; the next operation reloads it.
      if (git === mirror) git = null;
      mirror.free();
      throw e;
    }
  }

  /** `work` on the mirror, holding the lock, with what it wrote kept. */
  function writing<T>(work: (mirror: GitRepo) => T): Promise<T> {
    return lock("exclusive", async () => {
      const mirror = await sync();
      if (!mirror.open()) mirror.init();
      let out: T;
      try {
        out = work(mirror);
      } finally {
        await flush(mirror);
        if (!(await disk.read(POINTER))) {
          await disk.write(POINTER, new TextEncoder().encode(`${slot}\n`));
        }
      }
      announce();
      return out;
    });
  }

  async function reading<T>(work: (mirror: GitRepo) => T): Promise<T> {
    let mirror = git && fresh ? git : await lock("shared", sync);
    // No repo yet: the first visit makes an empty one.
    if (!mirror.open()) {
      await writing(() => undefined);
      mirror = git ?? (await lock("shared", sync));
    }
    return work(mirror);
  }

  async function run<K extends Kind>(
    request: Extract<Request, { kind: K }>,
  ): Promise<Results[K]> {
    const r = request as Request;
    const done = <V>(v: V) => v as unknown as Results[K];
    switch (r.kind) {
      case "getFile":
        return reading((g) => {
          const read = JSON.parse(g.get_file(r.path, r.at)) as FileRead;
          return done(
            read.kind === "file" ? { text: read.text, sha: read.sha } : null,
          );
        });
      case "getBytes":
        return reading((g) => done(g.get_bytes(r.path) ?? null));
      case "listDir":
        return reading((g) =>
          done((JSON.parse(g.list_dir(r.path)) as Listing).entries),
        );
      case "history":
        return reading((g) => {
          const page = JSON.parse(
            g.history(r.path, r.branch, r.until, r.page, r.perPage),
          ) as Revisions;
          return done(page.revisions.map(revisionOf));
        });
      case "revision":
        return reading((g) => {
          const read = JSON.parse(g.revision(r.commit)) as RevisionRead;
          return done(read.kind === "found" ? revisionOf(read.revision) : null);
        });
      case "blame":
        return reading((g) => {
          const read = JSON.parse(
            g.blame(r.stem, r.at, r.unsaved),
          ) as DeckBlame;
          return done(
            read.kind === "blamed" ? { at: read.at, cards: read.cards } : null,
          );
        });
      case "snapshots":
        return reading((g) =>
          done(
            (JSON.parse(g.snapshots(r.prefix)) as Snapshots).snapshots.map(
              snapshotOf,
            ),
          ),
        );
      case "takeSnapshot":
        return writing((g) => {
          const tagged = JSON.parse(
            g.take_snapshot(r.tag, r.label, r.commit, JSON.stringify(r.who)),
          ) as Tagged;
          if (tagged.kind === "conflict") throw new Conflict(tagged.message);
          return done(snapshotOf(tagged.snapshot));
        });
      case "dropSnapshot":
        return writing((g) => {
          g.drop_snapshot(r.tag);
          return done(null);
        });
      case "putFile":
        return writing((g) => {
          const wrote = JSON.parse(
            g.put_file(
              r.path,
              r.text,
              r.sha ?? undefined,
              r.message,
              JSON.stringify(r.who),
              r.branch,
              r.from,
            ),
          ) as Wrote;
          if (wrote.kind === "conflict") throw new Conflict(wrote.message);
          if (wrote.sha === undefined)
            throw new Error(`${r.path} was not written`);
          return done({ sha: wrote.sha, commit: wrote.commit });
        });
      case "decks":
        return reading((g) =>
          done(
            (JSON.parse(g.decks()) as DeckLineList).decks.map(
              (d): DeckLine => ({
                stem: d.stem,
                commit: d.commit,
                file:
                  d.text === undefined || d.sha === undefined
                    ? null
                    : { text: d.text, sha: d.sha },
                ...(d.parent === undefined ? {} : { parent: d.parent }),
                ...(d.upstream === undefined ? {} : { upstream: d.upstream }),
                ...(d.upstreamName === undefined
                  ? {}
                  : { upstreamName: d.upstreamName }),
              }),
            ),
          ),
        );
      case "deleteDeck":
        return writing((g) => {
          const deleted = JSON.parse(
            g.delete_deck(r.stem, r.commit),
          ) as DeckDeleted;
          if (deleted.kind === "conflict") throw new Conflict(deleted.message);
          return done(null);
        });
      case "migrate":
        return writing((g) => {
          g.migrate(r.from);
          return done(null);
        });
      case "branches":
        return reading((g) =>
          done(
            (JSON.parse(g.deck_branches(r.stem)) as DeckBranches).branches.map(
              branchOf,
            ),
          ),
        );
      case "createBranch":
        return writing((g) => {
          const made = JSON.parse(
            g.create_branch(r.stem, r.name, r.at),
          ) as BranchMade;
          if (made.kind === "conflict") throw new Conflict(made.message);
          return done(branchOf(made.branch));
        });
      case "deleteBranch":
        return writing((g) => {
          const deleted = JSON.parse(
            g.delete_branch(r.stem, r.name, r.commit),
          ) as BranchDeleted;
          if (deleted.kind === "conflict") throw new Conflict(deleted.message);
          return done(null);
        });
      // A plan writes the merged objects, which no ref names until a merge.
      case "planMerge":
        return writing((g) =>
          done(
            JSON.parse(
              g.plan_merge(r.stem, r.from, r.into, r.prefer),
            ) as MergePlan,
          ),
        );
      case "merge":
        return writing((g) =>
          done(
            JSON.parse(
              g.merge_branch(
                r.stem,
                r.from,
                r.into,
                r.intoTip,
                r.fromTip,
                r.prefer,
                JSON.stringify(r.who),
              ),
            ) as MergeDone,
          ),
        );
      case "deleteFile":
        return writing((g) => {
          const wrote = JSON.parse(
            g.delete_file(r.path, r.sha, r.message, JSON.stringify(r.who)),
          ) as Wrote;
          if (wrote.kind === "conflict") throw new Conflict(wrote.message);
          return done(null);
        });
      case "exportZip":
        return reading((g) => done(g.export_zip()));
      case "remote":
        return reading((g) => done(JSON.parse(g.remote()) as RemoteFacts));
      case "setRemote":
        return writing((g) => {
          g.set_remote(r.url);
          return done(null);
        });
      case "removeRemote":
        return writing((g) => {
          g.remove_remote();
          return done(null);
        });
      case "fetchRequest":
        return reading((g) => done(g.fetch_request(r.advertised) ?? null));
      case "fetchFinish":
        return writing((g) => {
          g.fetch_finish(r.advertised, r.response ?? undefined);
          return done(null);
        });
      case "integrate":
        return writing((g) =>
          done(
            JSON.parse(g.integrate(JSON.stringify(r.who), r.prefer)) as Synced,
          ),
        );
      case "adoptRemote":
        return writing((g) => {
          g.adopt_remote();
          return done(null);
        });
      case "pushRequest":
        return reading((g) => {
          const plan = g.push_request(r.advertised);
          const kind = plan.kind;
          const body = plan.body;
          plan.free();
          if (kind === "send") return done({ kind, body });
          if (kind === "behind" || kind === "nothing") return done({ kind });
          throw new Error(`a push plan of kind ${kind}`);
        });
      case "pushFinish":
        return writing((g) =>
          done(JSON.parse(g.push_finish(r.sent, r.response)) as PushOutcome),
        );
      case "followRequest":
        return reading((g) =>
          done(
            g.follow_request(
              r.theirs.url,
              r.theirs.deck,
              r.theirs.branch,
              r.advertised,
            ) ?? null,
          ),
        );
      case "followFinish":
        return writing((g) =>
          done(
            JSON.parse(
              g.follow_finish(
                r.theirs.url,
                r.theirs.deck,
                r.theirs.branch,
                r.advertised,
                r.response ?? undefined,
              ),
            ) as Fetched,
          ),
        );
      case "fork":
        return writing((g) =>
          done(
            JSON.parse(
              g.fork_deck(
                r.theirs.url,
                r.theirs.deck,
                r.theirs.branch,
                r.stem,
                JSON.stringify(r.who),
              ),
            ) as Forked,
          ),
        );
      case "following":
        return reading((g) => {
          const read = JSON.parse(g.following(r.stem)) as FollowingRead;
          return done(read.kind === "following" ? read.following : null);
        });
      case "unfollow":
        return writing((g) => {
          g.unfollow(r.stem, JSON.stringify(r.who));
          return done(null);
        });
      // A plan writes the merged objects, which no ref names until a merge.
      case "planTake":
        return writing((g) =>
          done(JSON.parse(g.plan_take(r.stem, r.prefer)) as MergePlan),
        );
      case "takeUpstream":
        return writing((g) =>
          done(
            JSON.parse(
              g.take_upstream(
                r.stem,
                r.intoTip,
                r.fromTip,
                r.prefer,
                JSON.stringify(r.who),
              ),
            ) as MergeDone,
          ),
        );
      case "importZip":
        return lock("exclusive", async () => {
          // Refused before storage is touched: a zip that is no repo, or one
          // `meldweb-git` cannot read, leaves this one as it was.
          const next = GitRepo.from_zip(r.bytes);
          const old = await pointer();
          const n = Number(/^r(\d+)$/.exec(old)?.[1] ?? "1");
          const into = `r${n + 1}`;
          await disk.removeDir(into);
          await flush(next, into);
          await disk.write(POINTER, new TextEncoder().encode(`${into}\n`));
          await disk.removeDir(old);
          git?.free();
          git = next;
          slot = into;
          fresh = true;
          announce();
          return done(null);
        });
    }
  }

  return {
    handle<K extends Kind>(
      request: Extract<Request, { kind: K }>,
    ): Promise<Reply<K>> {
      const answer = queue.then(async (): Promise<Reply<K>> => {
        try {
          return { kind: "ok", value: await run(request) };
        } catch (e) {
          const message = e instanceof Error ? e.message : String(e);
          return e instanceof Conflict
            ? { kind: "conflict", message }
            : { kind: "failed", message };
        }
      });
      queue = answer;
      return answer;
    },
    moved() {
      fresh = false;
    },
  };
}

/**
 * The steps of talking to the repo's one remote, `origin`, each atomic in the
 * repo's worker: the bytes are git's smart HTTP protocol, which the caller
 * carries to the host and back (`src/remote/`). Rust decides everything in
 * between; see `meldweb-git`'s `remote` module.
 */
export interface RemoteSteps {
  /** The remote's URL, and what waits to go to it or come from it. */
  facts(): Promise<RemoteFacts>;
  setRemote(url: string): Promise<void>;
  /** Forgets the remote and what was tracked of it; the repo stays. */
  removeRemote(): Promise<void>;
  /** The body to POST to git-upload-pack, or null when nothing is wanted. */
  fetchRequest(advertised: Uint8Array): Promise<Uint8Array | null>;
  /** Writes what the remote sent, and tracks its refs. */
  fetchFinish(
    advertised: Uint8Array,
    response: Uint8Array | null,
  ): Promise<void>;
  /** Brings each fetched branch into its own here: fast-forward, adopt or merge. */
  integrate(prefer?: Side): Promise<Synced>;
  /** Replaces this repo's branches and tags with the remote's. */
  adoptRemote(): Promise<void>;
  pushRequest(advertised: Uint8Array): Promise<PushPlan>;
  pushFinish(sent: Uint8Array, response: Uint8Array): Promise<PushOutcome>;
}

/** The browser's repo as a page holds it, with what only it can do. */
export interface LocalRepo extends Repo {
  /** The bare repo, zipped, as `git clone` reads it once unzipped. */
  exportZip(): Promise<Uint8Array>;
  /** Replaces the repo with the bare repo zipped in `bytes`; throws when it is refused. */
  importZip(bytes: Uint8Array): Promise<void>;
  readonly remote: RemoteSteps;
}

/** Carries one request to the core and its reply back. */
export type Call = <K extends Kind>(
  request: Extract<Request, { kind: K }>,
) => Promise<Reply<K>>;

/**
 * `clock` is the time commits are made at, in milliseconds: the browser's
 * own, or a seed's for a past it writes.
 */
export function createLocalRepo(
  call: Call,
  author: () => Author,
  clock: () => number = Date.now,
): LocalRepo {
  async function ask<K extends Kind>(
    request: Extract<Request, { kind: K }>,
  ): Promise<Results[K]> {
    const reply = await call(request);
    if (reply.kind === "ok") return reply.value;
    if (reply.kind === "conflict") throw new ConflictError(reply.message);
    throw new Error(reply.message);
  }
  const who = (): Who => {
    const { name, email } = author();
    return {
      name,
      email,
      seconds: Math.floor(clock() / 1000),
      offsetMinutes: -new Date(clock()).getTimezoneOffset(),
    };
  };
  return {
    where: "this browser",
    getFile: (path, ref) =>
      ask({ kind: "getFile", path, ...(ref === undefined ? {} : { at: ref }) }),
    getBytes: (path) => ask({ kind: "getBytes", path }),
    listDir: (path) => ask({ kind: "listDir", path }),
    history: (path, page = {}) =>
      ask({
        kind: "history",
        path,
        ...(page.branch === undefined ? {} : { branch: page.branch }),
        page: page.page ?? 1,
        perPage: Math.min(page.perPage ?? 100, 100),
        ...(page.until
          ? { until: Math.floor(Date.parse(page.until) / 1000) }
          : {}),
      }),
    revision: (commit) => ask({ kind: "revision", commit }),
    blame: (stem, at, unsaved) =>
      ask({
        kind: "blame",
        stem,
        at,
        ...(unsaved === undefined ? {} : { unsaved }),
      }),
    snapshots: (prefix) => ask({ kind: "snapshots", prefix }),
    takeSnapshot: ({ tag, label, commit }) =>
      ask({ kind: "takeSnapshot", tag, label, commit, who: who() }),
    async dropSnapshot(tag) {
      await ask({ kind: "dropSnapshot", tag });
    },
    async putFile(path, put) {
      return ask({
        kind: "putFile",
        path,
        text: put.text,
        sha: put.sha,
        message: put.message,
        who: who(),
        ...(put.branch === undefined ? {} : { branch: put.branch }),
        ...(put.from === undefined ? {} : { from: put.from }),
      });
    },
    decks: () => ask({ kind: "decks" }),
    async deleteDeck(stem, commit) {
      await ask({ kind: "deleteDeck", stem, commit });
    },
    async migrate(from) {
      await ask({ kind: "migrate", from });
    },
    branches: (stem) => ask({ kind: "branches", stem }),
    createBranch: (stem, name, at) =>
      ask({ kind: "createBranch", stem, name, at }),
    async deleteBranch(stem, name, commit) {
      await ask({ kind: "deleteBranch", stem, name, commit });
    },
    planMerge: (stem, { from, into, prefer }) =>
      ask({
        kind: "planMerge",
        stem,
        from,
        into,
        ...(prefer === undefined ? {} : { prefer }),
      }),
    merge: (stem, { from, into, intoTip, fromTip, prefer }) =>
      ask({
        kind: "merge",
        stem,
        from,
        into,
        intoTip,
        fromTip,
        ...(prefer === undefined ? {} : { prefer }),
        who: who(),
      }),
    async deleteFile(path, del) {
      await ask({ kind: "deleteFile", path, ...del, who: who() });
    },
    followRequest: (theirs, advertised) =>
      ask({ kind: "followRequest", theirs, advertised }),
    followFinish: (theirs, advertised, response) =>
      ask({ kind: "followFinish", theirs, advertised, response }),
    fork: (theirs, stem) => ask({ kind: "fork", theirs, stem, who: who() }),
    following: (stem) => ask({ kind: "following", stem }),
    async unfollow(stem) {
      await ask({ kind: "unfollow", stem, who: who() });
    },
    planTake: (stem, prefer) =>
      ask({
        kind: "planTake",
        stem,
        ...(prefer === undefined ? {} : { prefer }),
      }),
    takeUpstream: (stem, { intoTip, fromTip, prefer }) =>
      ask({
        kind: "takeUpstream",
        stem,
        intoTip,
        fromTip,
        ...(prefer === undefined ? {} : { prefer }),
        who: who(),
      }),
    exportZip: () => ask({ kind: "exportZip" }),
    async importZip(bytes) {
      await ask({ kind: "importZip", bytes });
    },
    remote: {
      facts: () => ask({ kind: "remote" }),
      async setRemote(url) {
        await ask({ kind: "setRemote", url });
      },
      async removeRemote() {
        await ask({ kind: "removeRemote" });
      },
      fetchRequest: (advertised) => ask({ kind: "fetchRequest", advertised }),
      async fetchFinish(advertised, response) {
        await ask({ kind: "fetchFinish", advertised, response });
      },
      integrate: (prefer) =>
        ask({
          kind: "integrate",
          who: who(),
          ...(prefer === undefined ? {} : { prefer }),
        }),
      async adoptRemote() {
        await ask({ kind: "adoptRemote" });
      },
      pushRequest: (advertised) => ask({ kind: "pushRequest", advertised }),
      pushFinish: (sent, response) =>
        ask({ kind: "pushFinish", sent, response }),
    },
  };
}
