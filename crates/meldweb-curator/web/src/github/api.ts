/**
 * The few GitHub REST calls Curator makes, as plain `fetch` to
 * api.github.com, which allows any origin (github-login-static-site.md,
 * "CORS"). Every call carries the user's token and retries once through a
 * refresh on a 401.
 */
import type { FileAt } from "../repo/api";
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

export type PublicFile =
  | ({ kind: "file" } & FileAt)
  | { kind: "no-file" }
  | { kind: "no-repo" };

/**
 * The user's account on GitHub: who they are and which repos the app
 * reaches. Only the dev mock's onboarding asks these. The Magic repo is the
 * browser's (ADR-0041), and GitHub holds it only as a remote, over git's own
 * protocol (`src/remote/`), never this REST API.
 */
export interface GitHubApi {
  user(): Promise<GitHubUser>;
  /** `GET /user/installations`: the app's installations this user can reach. */
  installations(): Promise<Installation[]>;
  /** `GET /user/installations/{id}/repositories`: what the token reaches there. */
  installationRepos(installation: number): Promise<RepoRef[]>;
  /** Whether the repo answers at all; a private repo without the app reads as absent. */
  repoExists(repo: RepoRef): Promise<boolean>;
}

/** Someone else's repo, read without a login, so only a public one answers. */
export interface PublicRepos {
  /** The file, `no-file` when the repo lacks it, `no-repo` when it is private or absent. */
  publicFile(repo: RepoRef, path: string): Promise<PublicFile>;
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

/** No token: the refresh cookie is gone or was never set. Log in again. */
export class LoggedOutError extends Error {
  override name = "LoggedOutError";
}

/**
 * The largest file the Contents API returns inline: past it GitHub sends
 * `content: ""` with `encoding: "none"`.
 */
export const CONTENTS_LIMIT = 1024 * 1024;

export function encodeBase64(text: string): string {
  const bytes = new TextEncoder().encode(text);
  let binary = "";
  for (let i = 0; i < bytes.length; i += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  }
  return btoa(binary);
}

export function decodeBase64(base64: string): string {
  return new TextDecoder().decode(base64Bytes(base64));
}

export function base64Bytes(base64: string): Uint8Array {
  const binary = atob(base64.replace(/\s/g, ""));
  return Uint8Array.from(binary, (c) => c.charCodeAt(0));
}

const encodePath = (path: string) =>
  path.split("/").map(encodeURIComponent).join("/");

const repoPath = (repo: RepoRef) =>
  `/repos/${encodeURIComponent(repo.owner)}/${encodeURIComponent(repo.name)}`;
const contents = (repo: RepoRef, path: string) =>
  `${repoPath(repo)}/contents/${encodePath(path)}`;

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

export interface GitHubApiOptions {
  auth: Auth;
  fetch?: typeof fetch;
  base?: string;
}

export function createGitHubApi(options: GitHubApiOptions): GitHubApi {
  const { auth } = options;
  const fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  const base = options.base ?? GITHUB_API;

  const send = (token: string, method: string, path: string) =>
    fetchImpl(`${base}${path}`, {
      method,
      // GitHub answers GETs with `private, max-age=60`; a cached read after a
      // save carries the old sha and turns our own commit into a conflict.
      cache: "no-store",
      headers: {
        Accept: "application/vnd.github+json",
        Authorization: `Bearer ${token}`,
        "X-GitHub-Api-Version": "2022-11-28",
      },
    });

  async function request(method: string, path: string): Promise<Response> {
    const token = await auth.token();
    if (!token) throw new LoggedOutError("not logged in to GitHub");
    const r = await send(token, method, path);
    if (r.status !== 401) return r;
    const again = await auth.refreshed(token);
    if (!again) throw new LoggedOutError("GitHub no longer accepts the login");
    return send(again, method, path);
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

  const account: GitHubApi = {
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
  };

  return account;
}

/** Anyone's public repo on GitHub, read as anyone may read it. */
export function createPublicRepos(
  options: { fetch?: typeof fetch; base?: string } = {},
): PublicRepos {
  const fetchImpl = options.fetch ?? globalThis.fetch.bind(globalThis);
  const base = options.base ?? GITHUB_API;
  // The app's token reaches only the repo it is installed on (ADR-0021), and
  // GitHub's docs make no exception for public repos, so someone else's is
  // read as anyone may read it.
  const anonymous = (path: string) =>
    fetchImpl(`${base}${path}`, {
      cache: "no-store",
      headers: {
        Accept: "application/vnd.github+json",
        "X-GitHub-Api-Version": "2022-11-28",
      },
    });

  return {
    async publicFile(repo, path) {
      const r = await anonymous(contents(repo, path));
      if (r.ok) return { kind: "file", ...(await fileOf(r, path)) };
      if (r.status !== 404) return fail(r);
      const there = await anonymous(repoPath(repo));
      if (there.ok) return { kind: "no-file" };
      if (there.status === 404) return { kind: "no-repo" };
      return fail(there);
    },
  };
}

/** A Contents API answer for `path` as the file's text and sha. */
async function fileOf(r: Response, path: string): Promise<FileAt> {
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
      `${path} is over 1 MB, which GitHub will not hand to Progress Engine`,
    );
  }
  return { text: decodeBase64(body.content), sha: body.sha };
}
