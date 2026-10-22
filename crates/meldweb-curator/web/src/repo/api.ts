/**
 * The Magic repo as every page reads and writes it, whichever backend holds
 * it: the bare git repo in this browser (ADR-0041), or GitHub through the dev
 * mock. A page holds a `Repo` and never asks which it is.
 */

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

export interface Repo {
  /** Where the repo is, as the deck list names it: `octocat/mtg`, or this browser. */
  readonly where: string;
  /** The file's page on the host's website; null for a repo no host has. */
  readonly onHost: ((path: string) => { host: string; url: string }) | null;
  /**
   * The file on the default branch, or at commit `ref` when given; `null`
   * when there is none there (or the repo is empty).
   */
  getFile(path: string, ref?: string): Promise<FileAt | null>;
  /** A file's bytes, such as a custom card's art; null when there is none. */
  getBytes(path: string): Promise<Uint8Array | null>;
  /**
   * The commits on the default branch that touched `path`, newest first, a
   * page of up to `perPage` (100 at most) at a time, none after `until`.
   */
  history(
    path: string,
    page?: { until?: string; page?: number; perPage?: number },
  ): Promise<Revision[]>;
  /** One commit, or `null` when the repo has none by that sha. */
  revision(commit: string): Promise<Revision | null>;
  /** The annotated tags whose names start with `prefix`. */
  snapshots(prefix: string): Promise<Snapshot[]>;
  /** Tags `commit` as `tag`. Throws `ConflictError` when the name is taken. */
  takeSnapshot(snapshot: {
    tag: string;
    label: string;
    commit: string;
  }): Promise<Snapshot>;
  dropSnapshot(tag: string): Promise<void>;
  /** One commit on the default branch. Throws `ConflictError` on a stale or missing sha. */
  putFile(path: string, put: PutFile): Promise<{ sha: string }>;
  /** One commit removing the file. Throws `ConflictError` on a stale or missing sha. */
  deleteFile(
    path: string,
    del: { message: string; sha: string },
  ): Promise<void>;
  /** A directory's entries, empty when it does not exist. */
  listDir(path: string): Promise<DirEntry[]>;
}

/**
 * The file moved in the repo since its sha was read, or a create found it
 * already there: another tab, device or person saved first. Saving merges
 * or asks (ADR-0021).
 */
export class ConflictError extends Error {
  override name = "ConflictError";
}
