/**
 * The few GitHub REST calls Curator makes, as plain `fetch` to
 * api.github.com, which allows any origin (github-login-static-site.md,
 * "CORS"). Every call carries the user's token and retries once through a
 * refresh on a 401.
 */
import type { Auth } from "./auth";

export const GITHUB_API = "https://api.github.com";

export interface RepoRef {
  owner: string;
  name: string;
}

export interface GitHubUser {
  login: string;
  id: number;
}

export interface Installation {
  id: number;
  account: string;
}

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
  /** The GitHub login that made it, when GitHub knows one. */
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

export interface GitHubApi {
  user(): Promise<GitHubUser>;
  /** `GET /user/installations`: the app's installations this user can reach. */
  installations(): Promise<Installation[]>;
  /** `GET /user/installations/{id}/repositories`: what the token reaches there. */
  installationRepos(installation: number): Promise<RepoRef[]>;
  /** Whether the repo answers at all; a private repo without the app reads as absent. */
  repoExists(repo: RepoRef): Promise<boolean>;
  /**
   * The file on the default branch, or at commit `ref` when given; `null`
   * when there is none there (or the repo is empty).
   */
  getFile(repo: RepoRef, path: string, ref?: string): Promise<FileAt | null>;
  /**
   * The commits on the default branch that touched `path`, newest first, a
   * page of up to `perPage` (100 at most) at a time, none after `until`.
   */
  history(
    repo: RepoRef,
    path: string,
    page?: { until?: string; page?: number; perPage?: number },
  ): Promise<Revision[]>;
  /** One commit, or `null` when the repo has none by that sha. */
  revision(repo: RepoRef, commit: string): Promise<Revision | null>;
  /** The annotated tags whose names start with `prefix`. */
  snapshots(repo: RepoRef, prefix: string): Promise<Snapshot[]>;
  /** Tags `commit` as `tag`. Throws `ConflictError` when the name is taken. */
  takeSnapshot(
    repo: RepoRef,
    snapshot: { tag: string; label: string; commit: string },
  ): Promise<Snapshot>;
  dropSnapshot(repo: RepoRef, tag: string): Promise<void>;
  /** One commit on the default branch. Throws `ConflictError` on a stale or missing sha. */
  putFile(repo: RepoRef, path: string, put: PutFile): Promise<{ sha: string }>;
  /** One commit removing the file. Throws `ConflictError` on a stale or missing sha. */
  deleteFile(
    repo: RepoRef,
    path: string,
    del: { message: string; sha: string },
  ): Promise<void>;
  /** A directory's entries, empty when it does not exist. */
  listDir(repo: RepoRef, path: string): Promise<DirEntry[]>;
}

export class GitHubError extends Error {
  override name = "GitHubError";
  constructor(
    readonly status: number,
    message: string,
  ) {
    super(message);
  }
}

/**
 * The file moved on GitHub since its sha was read, or a create found it
 * already there.
 *
 * GitHub's docs list both 409 and 422 for the Contents `PUT` without saying
 * which a stale sha gets. A 409 counts, and so does a 422 whose message is a
 * sha refusal (`isShaRefusal`); any other 422 is a malformed request, which
 * reloading or overwriting would not fix. NEEDS CONFIRMING against real
 * GitHub once the app exists (#125): pin the status and message it sends and
 * narrow this.
 */
export class ConflictError extends GitHubError {
  override name = "ConflictError";
}

/**
 * A 422 that is about the sha: GitHub's `"sha" wasn't supplied` for a create
 * over an existing file, or a `does not match` for a stale one.
 */
const isShaRefusal = (message: string) =>
  /"sha" wasn't supplied|does not match/.test(message);

/** No token: the refresh cookie is gone or was never set. Log in again. */
export class LoggedOutError extends Error {
  override name = "LoggedOutError";
}

/**
 * The largest file the Contents API returns inline: past it GitHub sends
 * `content: ""` with `encoding: "none"`.
 */
export const CONTENTS_LIMIT = 1024 * 1024;

/** Browsers refuse a keepalive request whose body passes 64 KiB. */
const KEEPALIVE_LIMIT = 60_000;

export function encodeBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

export function decodeBase64(base64: string): string {
  const binary = atob(base64.replace(/\s/g, ""));
  const bytes = Uint8Array.from(binary, (c) => c.charCodeAt(0));
  return new TextDecoder().decode(bytes);
}

const encodePath = (path: string) =>
  path.split("/").map(encodeURIComponent).join("/");

export interface GitHubApiOptions {
  auth: Auth;
  fetch?: typeof fetch;
  base?: string;
}

export function createGitHubApi(options: GitHubApiOptions): GitHubApi {
  const { auth } = options;
  const fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  const base = options.base ?? GITHUB_API;

  const send = (
    token: string,
    method: string,
    path: string,
    body: string | undefined,
    keepalive: boolean,
  ) =>
    fetchImpl(`${base}${path}`, {
      method,
      // GitHub answers GETs with `private, max-age=60`; a cached read after a
      // save carries the old sha and turns our own commit into a conflict.
      cache: "no-store",
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${token}`,
        "X-GitHub-Api-Version": "2022-11-28",
        ...(body === undefined ? {} : { "Content-Type": "application/json" }),
      },
      ...(body === undefined ? {} : { body }),
      ...(keepalive ? { keepalive: true } : {}),
    });

  async function request(
    method: string,
    path: string,
    body?: unknown,
    keepalive = false,
  ): Promise<Response> {
    const token = await auth.token();
    if (!token) throw new LoggedOutError("not logged in to GitHub");
    const json = body === undefined ? undefined : JSON.stringify(body);
    const ka = keepalive && json !== undefined && json.length < KEEPALIVE_LIMIT;
    const r = await send(token, method, path, json, ka);
    if (r.status !== 401) return r;
    const again = await auth.refreshed(token);
    if (!again) throw new LoggedOutError("GitHub no longer accepts the login");
    return send(again, method, path, json, ka);
  }

  /** GitHub's `message` for a failed request, or `""` when it sent none. */
  async function messageOf(r: Response): Promise<string> {
    try {
      const body = (await r.json()) as { message?: unknown };
      return typeof body.message === "string" ? body.message : "";
    } catch {
      return "";
    }
  }

  async function fail(r: Response, said?: string): Promise<never> {
    const message = said ?? (await messageOf(r));
    throw new GitHubError(
      r.status,
      `GitHub answered ${r.status}${message ? `: ${message}` : ""}`,
    );
  }

  async function json<T>(method: string, path: string): Promise<T> {
    const r = await request(method, path);
    if (!r.ok) return fail(r);
    return (await r.json()) as T;
  }

  async function paged<T>(
    path: string,
    pick: (page: Record<string, unknown>) => T[],
  ): Promise<T[]> {
    const all: T[] = [];
    for (let page = 1; ; page++) {
      const sep = path.includes("?") ? "&" : "?";
      const items = pick(
        await json<Record<string, unknown>>(
          "GET",
          `${path}${sep}per_page=100&page=${page}`,
        ),
      );
      all.push(...items);
      if (items.length < 100) return all;
    }
  }

  const repoPath = (repo: RepoRef) =>
    `/repos/${encodeURIComponent(repo.owner)}/${encodeURIComponent(repo.name)}`;
  const contents = (repo: RepoRef, path: string) =>
    `${repoPath(repo)}/contents/${encodePath(path)}`;

  async function sendJson<T>(method: string, path: string, body: unknown) {
    const r = await request(method, path, body);
    if (!r.ok) return fail(r);
    return (await r.json()) as T;
  }

  interface CommitJson {
    sha: string;
    commit: { message: string; committer: { date: string } | null };
    author: { login: string } | null;
  }
  const revisionOf = (c: CommitJson): Revision => ({
    commit: c.sha,
    message: c.commit.message,
    date: c.commit.committer?.date ?? "",
    author: c.author?.login ?? null,
  });

  interface TagObject {
    sha: string;
    tag: string;
    message: string;
    object: { sha: string; type: string };
    tagger?: { date?: string };
  }
  const snapshotOf = (t: TagObject): Snapshot => ({
    tag: t.tag,
    // Git ends a tag message with a newline whether or not it was sent one.
    label: t.message.replace(/\n+$/, ""),
    commit: t.object.sha,
    date: t.tagger?.date ?? "",
  });

  // GitHub says concurrent Contents writes conflict, so they go one at a time.
  let writes: Promise<unknown> = Promise.resolve();

  return {
    async user() {
      const u = await json<{ login: string; id: number }>("GET", "/user");
      return { login: u.login, id: u.id };
    },

    installations() {
      return paged("/user/installations", (p) =>
        (
          p.installations as { id: number; account: { login: string } | null }[]
        ).map((i) => ({ id: i.id, account: i.account?.login ?? "" })),
      );
    },

    installationRepos(installation) {
      return paged(`/user/installations/${installation}/repositories`, (p) =>
        (p.repositories as { name: string; owner: { login: string } }[]).map(
          (r) => ({ owner: r.owner.login, name: r.name }),
        ),
      );
    },

    async repoExists(repo) {
      const r = await request(
        "GET",
        `/repos/${encodeURIComponent(repo.owner)}/${encodeURIComponent(repo.name)}`,
      );
      if (r.status === 404) return false;
      if (!r.ok) return fail(r);
      return true;
    },

    async getFile(repo, path, ref) {
      const at = ref === undefined ? "" : `?ref=${encodeURIComponent(ref)}`;
      const r = await request("GET", `${contents(repo, path)}${at}`);
      if (r.status === 404) return null;
      if (!r.ok) return fail(r);
      const body = (await r.json()) as {
        type?: string;
        encoding?: string;
        content?: string;
        sha: string;
      };
      if (Array.isArray(body) || body.type !== "file") {
        throw new GitHubError(422, `${path} is not a file`);
      }
      // Anything but base64 is GitHub withholding the content, and reading
      // it as "" would open, and then save, an empty deck over the file.
      if (body.encoding !== "base64" || typeof body.content !== "string") {
        throw new GitHubError(
          413,
          `${path} is over 1 MB, which GitHub will not hand to Curator`,
        );
      }
      return { text: decodeBase64(body.content), sha: body.sha };
    },

    async history(repo, path, page = {}) {
      const query = new URLSearchParams({
        path,
        per_page: String(Math.min(page.perPage ?? 100, 100)),
        page: String(page.page ?? 1),
        ...(page.until ? { until: page.until } : {}),
      });
      const commits = await json<CommitJson[]>(
        "GET",
        `${repoPath(repo)}/commits?${query}`,
      );
      return commits.map(revisionOf);
    },

    async revision(repo, commit) {
      const r = await request(
        "GET",
        `${repoPath(repo)}/commits/${encodeURIComponent(commit)}`,
      );
      // GitHub answers a sha it has never seen with 422 as often as 404.
      if (r.status === 404 || r.status === 422) return null;
      if (!r.ok) return fail(r);
      return revisionOf((await r.json()) as CommitJson);
    },

    async snapshots(repo, prefix) {
      const refs = await json<
        { ref: string; object: { sha: string; type: string } }[]
      >(
        "GET",
        `${repoPath(repo)}/git/matching-refs/tags/${encodePath(prefix)}`,
      );
      // A lightweight tag has no label or date to show, so it is not one of
      // Curator's snapshots.
      const annotated = refs.filter((r) => r.object.type === "tag");
      const tags = await Promise.all(
        annotated.map((r) =>
          json<TagObject>("GET", `${repoPath(repo)}/git/tags/${r.object.sha}`),
        ),
      );
      return tags.map(snapshotOf);
    },

    async takeSnapshot(repo, { tag, label, commit }) {
      const object = await sendJson<TagObject>(
        "POST",
        `${repoPath(repo)}/git/tags`,
        { tag, message: label, object: commit, type: "commit" },
      );
      const r = await request("POST", `${repoPath(repo)}/git/refs`, {
        ref: `refs/tags/${tag}`,
        sha: object.sha,
      });
      if (r.status === 422) {
        const message = await messageOf(r);
        if (/already exists/i.test(message)) {
          throw new ConflictError(422, `there is already a snapshot ${tag}`);
        }
        return fail(r, message);
      }
      if (!r.ok) return fail(r);
      return snapshotOf(object);
    },

    async dropSnapshot(repo, tag) {
      const r = await request(
        "DELETE",
        `${repoPath(repo)}/git/refs/tags/${encodePath(tag)}`,
      );
      // GitHub answers a tag that is already gone with 422 or 404; either way
      // it is gone, which is what was asked.
      if (!r.ok && r.status !== 404 && r.status !== 422) return fail(r);
    },

    putFile(repo, path, put) {
      const run = writes.then(async () => {
        const r = await request(
          "PUT",
          contents(repo, path),
          {
            message: put.message,
            content: encodeBase64(put.text),
            ...(put.sha === null ? {} : { sha: put.sha }),
          },
          put.keepalive ?? false,
        );
        if (r.status === 409) {
          throw new ConflictError(r.status, `${path} changed on GitHub (409)`);
        }
        if (r.status === 422) {
          const message = await messageOf(r);
          if (isShaRefusal(message)) {
            throw new ConflictError(
              r.status,
              `${path} changed on GitHub (422)`,
            );
          }
          return fail(r, message);
        }
        if (!r.ok) return fail(r);
        const body = (await r.json()) as { content: { sha: string } };
        return { sha: body.content.sha };
      });
      writes = run.catch(() => undefined);
      return run;
    },

    deleteFile(repo, path, del) {
      const run = writes.then(async () => {
        const r = await request("DELETE", contents(repo, path), del);
        // A missing file is a 404 here, which for a deck means someone
        // else got there first: the same as a stale sha.
        if (r.status === 409 || r.status === 404) {
          throw new ConflictError(
            r.status,
            `${path} changed on GitHub (${r.status})`,
          );
        }
        if (r.status === 422) {
          const message = await messageOf(r);
          if (isShaRefusal(message)) {
            throw new ConflictError(
              r.status,
              `${path} changed on GitHub (422)`,
            );
          }
          return fail(r, message);
        }
        if (!r.ok) return fail(r);
      });
      writes = run.catch(() => undefined);
      return run;
    },

    async listDir(repo, path) {
      const r = await request("GET", contents(repo, path));
      if (r.status === 404) return [];
      if (!r.ok) return fail(r);
      const body: unknown = await r.json();
      if (!Array.isArray(body)) return [];
      return (body as DirEntry[]).map(({ name, path, sha, type }) => ({
        name,
        path,
        sha,
        type,
      }));
    },
  };
}
