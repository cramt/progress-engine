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

export interface GitHubApi {
  user(): Promise<GitHubUser>;
  /** `GET /user/installations`: the app's installations this user can reach. */
  installations(): Promise<Installation[]>;
  /** `GET /user/installations/{id}/repositories`: what the token reaches there. */
  installationRepos(installation: number): Promise<RepoRef[]>;
  /** Whether the repo answers at all; a private repo without the app reads as absent. */
  repoExists(repo: RepoRef): Promise<boolean>;
  /** The file on the default branch, or `null` when there is none (or the repo is empty). */
  getFile(repo: RepoRef, path: string): Promise<FileAt | null>;
  /** One commit on the default branch. Throws `ConflictError` on a stale or missing sha. */
  putFile(repo: RepoRef, path: string, put: PutFile): Promise<{ sha: string }>;
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
 * which a stale sha gets, so both count. NEEDS CONFIRMING against real GitHub
 * once the app exists (#125): pin the one it sends and narrow this.
 */
export class ConflictError extends GitHubError {
  override name = "ConflictError";
}

/** No token: the refresh cookie is gone or was never set. Log in again. */
export class LoggedOutError extends Error {
  override name = "LoggedOutError";
}

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

  async function fail(r: Response): Promise<never> {
    let message = `GitHub answered ${r.status}`;
    try {
      const body = (await r.json()) as { message?: unknown };
      if (typeof body.message === "string") message += `: ${body.message}`;
    } catch {
      // no JSON body
    }
    throw new GitHubError(r.status, message);
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

  const contents = (repo: RepoRef, path: string) =>
    `/repos/${encodeURIComponent(repo.owner)}/${encodeURIComponent(repo.name)}/contents/${encodePath(path)}`;

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

    async getFile(repo, path) {
      const r = await request("GET", contents(repo, path));
      if (r.status === 404) return null;
      if (!r.ok) return fail(r);
      const body = (await r.json()) as {
        type?: string;
        content?: string;
        sha: string;
      };
      if (Array.isArray(body) || body.type !== "file") {
        throw new GitHubError(422, `${path} is not a file`);
      }
      return { text: decodeBase64(body.content ?? ""), sha: body.sha };
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
        if (r.status === 409 || r.status === 422) {
          throw new ConflictError(
            r.status,
            `${path} changed on GitHub (${r.status})`,
          );
        }
        if (!r.ok) return fail(r);
        const body = (await r.json()) as { content: { sha: string } };
        return { sha: body.content.sha };
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
