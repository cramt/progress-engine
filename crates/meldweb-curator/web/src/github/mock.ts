/**
 * GitHub and the auth worker, faked in the page for `pnpm dev` without an app
 * and for vitest. It is a `fetch`: the real auth and API clients run against
 * it unchanged, so the app chooses it once (`connect.ts`) and never asks
 * again. It answers exactly the calls those clients make, with GitHub's
 * shapes, and refuses a stale sha with 409.
 *
 * Tokens rotate like the real ones: each refresh makes the previous access
 * token answer 401.
 */

import { decodeBase64, encodeBase64, GITHUB_API } from "./api";
import { AUTH_ENDPOINTS } from "./auth";
import { MAGIC_REPO } from "./repo";

export interface MockCommit {
  path: string;
  message: string;
  sha: string;
  /** Made by `editOnGitHub`, not through the API. */
  outOfBand: boolean;
}

export interface MockRequest {
  method: string;
  url: string;
  keepalive: boolean;
  cache: RequestCache | undefined;
}

interface State {
  login: string;
  loggedIn: boolean;
  repo: {
    installed: boolean;
    files: Record<string, { text: string; sha: string }>;
  } | null;
  seq: number;
  token: string | null;
  commits: MockCommit[];
}

export interface MockOptions {
  login?: string;
  /** `ready`: `mtg` exists with the app on it (default). The others start onboarding. */
  start?: "ready" | "no-repo" | "no-install";
  /** Seed files by path, e.g. `decks/lantern.deck.toml`. */
  files?: Record<string, string>;
  loggedIn?: boolean;
  /** Keeps the fake across reloads, e.g. `sessionStorage`. */
  storage?: Pick<Storage, "getItem" | "setItem"> | null;
  storageKey?: string;
  now?: () => number;
  /** For anything that is neither GitHub nor the worker (Scryfall). */
  passthrough?: typeof fetch;
}

export interface MockGitHub {
  fetch: typeof fetch;
  /** What the worker's login round trip would leave behind. */
  logIn(): void;
  /** The user made `mtg` on github.com/new. */
  createRepo(): void;
  /** The user installed the app on `mtg`. */
  installApp(): void;
  /** A commit made outside Curator, as if on github.com. */
  editOnGitHub(path: string, text: string): void;
  file(path: string): { text: string; sha: string } | undefined;
  commits(): readonly MockCommit[];
  requests(): readonly MockRequest[];
  /** Refreshes the worker has answered. */
  refreshes(): number;
}

export const MOCK_APP_SLUG = "meldweb-curator-mock";

const json = (status: number, body: unknown) =>
  new Response(JSON.stringify(body), {
    status,
    headers: { "Content-Type": "application/json" },
  });
const notFound = () => json(404, { message: "Not Found" });

export function createMockGitHub(options: MockOptions = {}): MockGitHub {
  const key = options.storageKey ?? "meldweb-mock-github";
  const storage = options.storage ?? null;
  const now = options.now ?? Date.now;
  const passthrough = options.passthrough ?? globalThis.fetch?.bind(globalThis);
  const requests: MockRequest[] = [];
  let refreshes = 0;

  const fresh = (): State => {
    const start = options.start ?? "ready";
    const st: State = {
      login: options.login ?? "octocat",
      loggedIn: options.loggedIn ?? true,
      repo: null,
      seq: 0,
      token: null,
      commits: [],
    };
    if (start !== "no-repo") {
      st.repo = { installed: start === "ready", files: {} };
      for (const [path, text] of Object.entries(options.files ?? {})) {
        st.repo.files[path] = { text, sha: nextSha(st) };
      }
    }
    return st;
  };

  function nextSha(st: State): string {
    st.seq += 1;
    return st.seq.toString(16).padStart(40, "0");
  }

  const stored = storage?.getItem(key);
  const state: State = stored ? (JSON.parse(stored) as State) : fresh();
  const persist = () => storage?.setItem(key, JSON.stringify(state));
  persist();

  const reachable = (owner: string, name: string) =>
    owner.toLowerCase() === state.login.toLowerCase() &&
    name.toLowerCase() === MAGIC_REPO &&
    state.repo?.installed === true;

  function contentsGet(path: string): Response {
    const files = state.repo?.files ?? {};
    const file = files[path];
    if (file) {
      return json(200, {
        type: "file",
        name: path.slice(path.lastIndexOf("/") + 1),
        path,
        sha: file.sha,
        encoding: "base64",
        // GitHub wraps its base64 at 60 columns.
        content: encodeBase64(file.text).replace(/(.{60})/g, "$1\n"),
      });
    }
    const prefix = path === "" ? "" : `${path}/`;
    const entries = Object.entries(files)
      .filter(
        ([p]) => p.startsWith(prefix) && !p.slice(prefix.length).includes("/"),
      )
      .map(([p, f]) => ({
        type: "file",
        name: p.slice(prefix.length),
        path: p,
        sha: f.sha,
      }));
    if (entries.length === 0) {
      return json(404, {
        message:
          Object.keys(files).length === 0
            ? "This repository is empty."
            : "Not Found",
      });
    }
    return json(200, entries);
  }

  function contentsPut(path: string, body: string | null): Response {
    const files = state.repo?.files;
    if (!files) return notFound();
    const put = JSON.parse(body ?? "{}") as {
      message?: string;
      content?: string;
      sha?: string;
    };
    if (typeof put.message !== "string" || typeof put.content !== "string") {
      return json(422, { message: "Invalid request." });
    }
    const existing = files[path];
    if (existing && put.sha === undefined) {
      return json(422, {
        message: `Invalid request.\n\n"sha" wasn't supplied.`,
      });
    }
    if (put.sha !== undefined && put.sha !== existing?.sha) {
      return json(409, { message: `${path} does not match ${put.sha}` });
    }
    const sha = nextSha(state);
    files[path] = { text: decodeBase64(put.content), sha };
    state.commits.push({ path, message: put.message, sha, outOfBand: false });
    persist();
    return json(existing ? 200 : 201, {
      content: { path, sha },
      commit: { sha: nextSha(state), message: put.message },
    });
  }

  function api(
    method: string,
    url: URL,
    auth: string | null,
    body: string | null,
  ) {
    if (!state.token || auth !== `Bearer ${state.token}`) {
      return json(401, { message: "Bad credentials" });
    }
    const p = url.pathname;
    if (method === "GET" && p === "/user") {
      return json(200, { login: state.login, id: 1 });
    }
    const page = Number(url.searchParams.get("page") ?? "1");
    if (method === "GET" && p === "/user/installations") {
      const installations =
        state.repo?.installed && page === 1
          ? [{ id: 1, account: { login: state.login } }]
          : [];
      return json(200, { total_count: installations.length, installations });
    }
    if (method === "GET" && p === "/user/installations/1/repositories") {
      if (!state.repo?.installed) return notFound();
      const repositories =
        page === 1
          ? [
              {
                name: MAGIC_REPO,
                full_name: `${state.login}/${MAGIC_REPO}`,
                owner: { login: state.login },
                default_branch: "main",
              },
            ]
          : [];
      return json(200, { total_count: 1, repositories });
    }
    const repo = p.match(/^\/repos\/([^/]+)\/([^/]+)$/);
    if (method === "GET" && repo) {
      const [, owner = "", name = ""] = repo;
      // A public repo answers whether or not the app is on it.
      return state.repo &&
        owner.toLowerCase() === state.login.toLowerCase() &&
        name.toLowerCase() === MAGIC_REPO
        ? json(200, { name: MAGIC_REPO, owner: { login: state.login } })
        : notFound();
    }
    const contents = p.match(/^\/repos\/([^/]+)\/([^/]+)\/contents\/?(.*)$/);
    if (contents) {
      const [, owner = "", name = "", rest = ""] = contents;
      if (!reachable(owner, name)) return notFound();
      const path = rest.split("/").map(decodeURIComponent).join("/");
      if (method === "GET") return contentsGet(path);
      if (method === "PUT") return contentsPut(path, body);
    }
    return notFound();
  }

  function worker(method: string, path: string): Response {
    if (method === "POST" && path === AUTH_ENDPOINTS.refresh) {
      refreshes += 1;
      if (!state.loggedIn) {
        persist();
        return json(401, { message: "no session" });
      }
      state.token = `ghu_mock_${nextSha(state).slice(-8)}`;
      persist();
      return json(200, {
        access_token: state.token,
        expires_at: now() + 8 * 60 * 60 * 1000,
      });
    }
    if (method === "GET" && path === AUTH_ENDPOINTS.app) {
      return json(200, {
        app_slug: MOCK_APP_SLUG,
        install_url: `https://github.com/apps/${MOCK_APP_SLUG}/installations/new`,
      });
    }
    if (method === "POST" && path === AUTH_ENDPOINTS.logout) {
      state.loggedIn = false;
      state.token = null;
      persist();
      return new Response(null, { status: 204 });
    }
    return notFound();
  }

  const mockFetch = async (
    input: RequestInfo | URL,
    init?: RequestInit,
  ): Promise<Response> => {
    const href =
      input instanceof Request
        ? input.url
        : input instanceof URL
          ? input.href
          : input;
    const url = new URL(href, "http://curator.invalid");
    const method = (init?.method ?? "GET").toUpperCase();
    const headers = new Headers(init?.headers);
    const body = typeof init?.body === "string" ? init.body : null;
    const isApi = href.startsWith(GITHUB_API);
    const isWorker = !isApi && url.pathname.startsWith("/api/auth/");
    if (!isApi && !isWorker) {
      if (!passthrough)
        throw new TypeError(`mock GitHub has no route for ${href}`);
      return passthrough(input, init);
    }
    requests.push({
      method,
      url: href,
      keepalive: init?.keepalive === true,
      cache: init?.cache,
    });
    return isApi
      ? api(method, url, headers.get("Authorization"), body)
      : worker(method, url.pathname);
  };

  return {
    fetch: mockFetch as typeof fetch,
    logIn() {
      state.loggedIn = true;
      persist();
    },
    createRepo() {
      state.repo ??= { installed: false, files: {} };
      persist();
    },
    installApp() {
      state.repo ??= { installed: false, files: {} };
      state.repo.installed = true;
      persist();
    },
    editOnGitHub(path, text) {
      state.repo ??= { installed: false, files: {} };
      const sha = nextSha(state);
      state.repo.files[path] = { text, sha };
      state.commits.push({
        path,
        message: `edit ${path}`,
        sha,
        outOfBand: true,
      });
      persist();
    },
    file: (path) => state.repo?.files[path],
    commits: () => state.commits,
    requests: () => requests,
    refreshes: () => refreshes,
  };
}
