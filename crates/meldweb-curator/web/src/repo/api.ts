/**
 * The Magic repo as every page reads and writes it: the bare git repo in this
 * browser (ADR-0041), in the worker that holds it, or in memory in tests and
 * the dev mock. A page holds a `Repo` and never asks which it is.
 *
 * Repo version 2 has a line of history per deck. `main` holds `VERSION`,
 * `collection.toml`, `wanted.toml`, `meldweb.toml` and `cards/`; each deck is
 * the branch `decks/<stem>/<branch>`, every deck has `main`, and its tree
 * holds `<stem>.deck.toml` and `<stem>.criteria.toml`. A file call names the
 * branch it is on, `main` when it names none.
 */

import type {
  CardBlame,
  Fetched,
  Following,
  Forked,
  MergeDone,
  MergePlan,
  SaveAsk,
  Saved,
  Settle,
  Side,
  UpstreamFacts,
} from "../deck.gen";

export interface FileAt {
  text: string;
  /** The blob sha, which a later `putFile` sends back as its guard. */
  sha: string;
}

export interface DirEntry {
  name: string;
  path: string;
  sha: string;
  type: "file" | "dir" | "symlink" | "submodule";
}

export interface PutFile {
  text: string;
  message: string;
  /** The blob sha being replaced; `null` creates the file and refuses if it exists. */
  sha: string | null;
  /** Lets the request outlive the page (`pagehide`), when the body is small enough. */
  keepalive?: boolean;
  /** The branch to commit on, `main` when absent: `decks/lantern/main`. */
  branch?: string;
  /**
   * For a branch this write makes: the commit it starts from, so the new
   * branch's history is that commit's, which is how a deck is forked.
   */
  from?: string;
}

/** A deck, as its `main` branch has it. */
export interface DeckLine {
  stem: string;
  /** The tip of `decks/<stem>/main`. */
  commit: string;
  /** `<stem>.deck.toml` there, and its blob sha; absent when it holds none. */
  file: FileAt | null;
  /** The deck it was forked from, as its history says. */
  parent?: string;
  /** The deck in another player's repo it follows, as `meldweb.toml` says. */
  upstream?: UpstreamFacts;
  /** That deck's name, as last fetched. */
  upstreamName?: string;
}

/** A deck in another player's repo: their repo's URL, its stem, its branch. */
export interface TheirDeck {
  url: string;
  deck: string;
  branch: string;
}

/** One commit that touched a file, as its history lists it. */
export interface Revision {
  /** The commit sha, which `getFile` reads the file at. */
  commit: string;
  message: string;
  /** When it was committed, ISO 8601. */
  date: string;
  /** Who made it: a GitHub login, or the author's name; null when unknown. */
  author: string | null;
}

/** An annotated tag: a version of a file kept under a name of the user's. */
export interface Snapshot {
  /** The tag's name without `refs/tags/`, e.g. `decks/lantern/fnm`. */
  tag: string;
  /** What the user called it. */
  label: string;
  /** The commit it points at. */
  commit: string;
  /** When it was taken, ISO 8601. */
  date: string;
}

/** One of a deck's branches, `decks/<stem>/<name>`, as the picker lists it. */
export interface Branch {
  /** Its own name: `budget` for `decks/lantern/budget`. `main` is the built deck. */
  name: string;
  /** Its tip. */
  commit: string;
  /** When its tip was committed, ISO 8601. */
  date: string;
  /** Commits on it that the deck's `main` lacks; deleting it loses them. */
  unmerged: number;
  /** Commits on `main` that it lacks. */
  behind: number;
}

/**
 * Merging one of a deck's branches into another: which, and how its clashes
 * are settled, under the guard of the conflict the picks were made against.
 */
export interface MergeAsk {
  from: string;
  into: string;
  settle?: Settle;
}

export interface Repo {
  /** Where the repo is, as the deck list names it: `octocat/mtg`, or this browser. */
  readonly where: string;
  /**
   * The file on `main`, or at `ref` when given, a commit's sha or a branch's
   * name; `null` when there is none there (or the repo is empty).
   */
  getFile(path: string, ref?: string): Promise<FileAt | null>;
  /** A file's bytes, such as a custom card's art; null when there is none. */
  getBytes(path: string): Promise<Uint8Array | null>;
  /**
   * The commits on `branch` (default `main`) that touched `path`, newest
   * first, a page of up to `perPage` (100 at most) at a time, none after
   * `until`.
   */
  history(
    path: string,
    page?: { until?: string; page?: number; perPage?: number; branch?: string },
  ): Promise<Revision[]>;
  /** One commit, or `null` when the repo has none by that sha. */
  revision(commit: string): Promise<Revision | null>;
  /**
   * Every card of the deck `stem` at `at` (a commit, or a branch such as
   * `decks/lantern/main`) with the commit that last changed it and the one
   * it came in at, in the order of the deck's cards (ADR-0041). `unsaved` is
   * the editor's text on top of `at`, whose cards follow it instead. `null`
   * when there is no deck there.
   */
  blame(stem: string, at: string, unsaved?: string): Promise<BlameRead | null>;
  /** The annotated tags whose names start with `prefix`. */
  snapshots(prefix: string): Promise<Snapshot[]>;
  /** Tags `commit` as `tag`. Throws `ConflictError` when the name is taken. */
  takeSnapshot(snapshot: {
    tag: string;
    label: string;
    commit: string;
  }): Promise<Snapshot>;
  dropSnapshot(tag: string): Promise<void>;
  /**
   * One commit on `put.branch` (default `main`), and its new tip. Throws
   * `ConflictError` on a stale or missing sha.
   */
  putFile(path: string, put: PutFile): Promise<{ sha: string; commit: string }>;
  /**
   * One save of an editor's text (ADR-0021), in one step under the repo's
   * lock: committed when the file has not moved since the text it was made
   * from, otherwise stopped or merged into the file as it is now (a deck by
   * card, anything else by lines) and committed, with the message the file
   * kind's diff says. Every outcome is a value: `moved`, `clashed` and
   * `refused` commit nothing.
   */
  save(ask: SaveAsk): Promise<Saved>;
  /** One commit removing the file. Throws `ConflictError` on a stale or missing sha. */
  deleteFile(
    path: string,
    del: { message: string; sha: string },
  ): Promise<void>;
  /** A directory's entries on `main`, empty when it does not exist. */
  listDir(path: string): Promise<DirEntry[]>;
  /** Every deck, `decks/<stem>/main`, by stem. */
  decks(): Promise<DeckLine[]>;
  /**
   * Deletes the deck, every branch and snapshot of it, while its `main` is
   * still at `commit`; throws `ConflictError` otherwise.
   */
  deleteDeck(stem: string, commit: string): Promise<void>;
  /** The deck's branches, `main` first and then the newest first. */
  branches(stem: string): Promise<Branch[]>;
  /**
   * Makes the deck's branch `name` at `at`, a commit holding the deck's file.
   * Throws `ConflictError` when the name is taken.
   */
  createBranch(stem: string, name: string, at: string): Promise<Branch>;
  /** Deletes the branch while it is at `commit`; throws `ConflictError` otherwise. */
  deleteBranch(stem: string, name: string, commit: string): Promise<void>;
  /** What merging `ask.from` into `ask.into` would do; nothing moves. */
  planMerge(stem: string, ask: MergeAsk): Promise<MergePlan>;
  /** Merges as `planMerge` said, while the two are still at the plan's tips. */
  merge(
    stem: string,
    ask: MergeAsk & { intoTip: string; fromTip: string },
  ): Promise<MergeDone>;
  /** Takes the repo from version `from` to the next, all or nothing. */
  migrate(from: number): Promise<void>;
  /**
   * Fetching another player's deck over git (ADR-0041), whose bytes the
   * caller carries (`src/remote/follow.ts`): the body to POST to their
   * git-upload-pack for that deck's line alone, or null when it is all here.
   */
  followRequest(
    theirs: TheirDeck,
    advertised: Uint8Array,
  ): Promise<Uint8Array | null>;
  /** Takes in their answer and tracks their deck's line. */
  followFinish(
    theirs: TheirDeck,
    advertised: Uint8Array,
    response: Uint8Array | null,
  ): Promise<Fetched>;
  /** Makes `stem` a fork of their deck as fetched, following it. */
  fork(theirs: TheirDeck, stem: string): Promise<Forked>;
  /** The deck against the deck it follows; null when it follows none. */
  following(stem: string): Promise<Following | null>;
  /** Stops the deck following anything; its history stays. */
  unfollow(stem: string): Promise<void>;
  /** What taking the followed deck's changes would do; nothing moves. */
  planTake(stem: string, settle?: Settle): Promise<MergePlan>;
  /** Takes them as `planTake` said, while both are still at its tips. */
  takeUpstream(
    stem: string,
    ask: { intoTip: string; fromTip: string; settle?: Settle },
  ): Promise<MergeDone>;
}

/** A deck's cards blamed at a commit. */
export interface BlameRead {
  /** The commit blamed. */
  at: string;
  cards: CardBlame[];
}

export type {
  CardBlame,
  Fetched,
  Following,
  Forked,
  MergeDone,
  MergePlan,
  SaveAsk,
  Saved,
  Settle,
  Side,
};

/**
 * The file moved in the repo since its sha was read, or a create found it
 * already there: another tab, device or person wrote first.
 */
export class ConflictError extends Error {
  override name = "ConflictError";
}
