/**
 * GitHub and the auth worker, faked in the page for `pnpm dev` without an app
 * and for vitest. It is a `fetch`: the real auth and API clients run against
 * it unchanged, so the app chooses it once (`connect.ts`) and never asks
 * again. It answers exactly the calls those clients make, with GitHub's
 * shapes: the login, the onboarding's installations, and other players'
 * public repos. The Magic repo itself is the browser's (ADR-0041); in the
 * mock it is one held in memory (`repo/memory.ts`), seeded by `mockSeed.ts`.
 *
 * Tokens rotate like the real ones: each refresh makes the previous access
 * token answer 401.
 */

import { CONTENTS_LIMIT, encodeBase64, GITHUB_API } from "./api";
import { AUTH_ENDPOINTS } from "./auth";
import { MAGIC_REPO } from "./repo";

export interface MockRequest {
  method: string;
  url: string;
  keepalive: boolean;
  cache: RequestCache | undefined;
}

interface State {
  login: string;
  loggedIn: boolean;
  /** `mtg` on GitHub, which onboarding makes and installs the app on. */
  repo: { installed: boolean } | null;
  seq: number;
  token: string | null;
}

/** A commit made before the mock starts, so a fresh dev session has a past. */
export interface SeedCommit {
  path: string;
  text: string;
  message: string;
  /** ISO 8601. */
  date: string;
}

export interface MockOptions {
  login?: string;
  /** `ready`: `mtg` exists with the app on it (default). The others start onboarding. */
  start?: "ready" | "no-repo" | "no-install";
  /** Other players' public `mtg` repos, by login: each its files by path. */
  others?: Record<string, Record<string, string>>;
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
    return {
      login: options.login ?? "octocat",
      loggedIn: options.loggedIn ?? true,
      repo: start === "no-repo" ? null : { installed: start === "ready" },
      seq: 0,
      token: null,
    };
  };

  function nextSha(st: State): string {
    st.seq += 1;
    return st.seq.toString(16).padStart(40, "0");
  }

  const stored = storage?.getItem(key);
  const state: State = stored ? (JSON.parse(stored) as State) : fresh();
  const persist = () => storage?.setItem(key, JSON.stringify(state));
  persist();

  function fileGet(path: string, file: { text: string; sha: string }) {
    const size = new TextEncoder().encode(file.text).length;
    return json(200, {
      type: "file",
      name: path.slice(path.lastIndexOf("/") + 1),
      path,
      sha: file.sha,
      size,
      // Past 1 MB GitHub sends no content and says so in `encoding`.
      ...(size > CONTENTS_LIMIT
        ? { encoding: "none", content: "" }
        : {
            encoding: "base64",
            // GitHub wraps its base64 at 60 columns.
            content: encodeBase64(file.text).replace(/(.{60})/g, "$1\n"),
          }),
    });
  }

  const others = new Map(
    Object.entries(options.others ?? {}).map(([login, files]) => [
      login.toLowerCase(),
      { login, files },
    ]),
  );

  /**
   * Another player's public repo, which answers a read with or without a
   * token; `null` for anything else.
   */
  function publicGet(method: string, path: string): Response | null {
    const m = path.match(/^\/repos\/([^/]+)\/([^/]+)(?:\/contents\/(.*))?$/);
    if (method !== "GET" || !m || m[2]?.toLowerCase() !== MAGIC_REPO) {
      return null;
    }
    const other = others.get((m[1] ?? "").toLowerCase());
    if (!other) return null;
    if (m[3] === undefined) {
      return json(200, { name: MAGIC_REPO, owner: { login: other.login } });
    }
    const file = m[3].split("/").map(decodeURIComponent).join("/");
    const text = other.files[file];
    return text === undefined
      ? notFound()
      : fileGet(file, { text, sha: `${other.login}:${file}` });
  }

  function api(method: string, url: URL, auth: string | null) {
    const open = publicGet(method, url.pathname);
    if (open) return open;
    // Asked without a token, a repo that is missing or private is not found.
    if (auth === null && url.pathname.startsWith("/repos/")) return notFound();
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
      ? api(method, url, headers.get("Authorization"))
      : worker(method, url.pathname);
  };

  return {
    fetch: mockFetch as typeof fetch,
    logIn() {
      state.loggedIn = true;
      persist();
    },
    createRepo() {
      state.repo ??= { installed: false };
      persist();
    },
    installApp() {
      state.repo ??= { installed: false };
      state.repo.installed = true;
      persist();
    },
    requests: () => requests,
    refreshes: () => refreshes,
  };
}
