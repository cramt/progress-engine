/**
 * The one place the app chooses between real GitHub and the mock. Everything
 * after this takes the returned `auth`, `api` and `onboarding` and does not ask
 * which it got.
 *
 * `VITE_MOCK_GITHUB=1 pnpm dev` fakes GitHub and the worker in the page, seeded
 * with Lantern (and six weeks of its history) and Loam, and `rival`'s public
 * repo to trade against; `no-repo` or `no-install` start at onboarding. The
 * fake lives in sessionStorage, so a reload keeps its commits, and it is on
 * `window.__mockGitHub` for simulating an edit on github.com from the console.
 */
import { createGitHubApi, type GitHubApi } from "./api";
import { type Auth, createAuth } from "./auth";
import type { MockGitHub } from "./mock";
import { githubOnboarding, type Onboarding } from "./onboarding";

export interface Connection {
  auth: Auth;
  api: GitHubApi;
  onboarding: Onboarding;
  /** Only for dev tooling; app code must not branch on it. */
  mock: MockGitHub | null;
}

let connection: Promise<Connection> | null = null;

export function connect(): Promise<Connection> {
  connection ??= choose();
  return connection;
}

async function choose(): Promise<Connection> {
  const mode = import.meta.env.VITE_MOCK_GITHUB;
  if (!mode) {
    const auth = createAuth();
    return {
      auth,
      api: createGitHubApi({ auth }),
      onboarding: githubOnboarding,
      mock: null,
    };
  }
  const [{ createMockGitHub }, seed] = await Promise.all([
    import("./mock"),
    import("./mockSeed"),
  ]);
  const mock = createMockGitHub({
    start: mode === "no-repo" || mode === "no-install" ? mode : "ready",
    files: mode === "no-repo" || mode === "no-install" ? {} : seed.files,
    history:
      mode === "no-repo" || mode === "no-install"
        ? []
        : seed.history(Date.now()),
    others: seed.others,
    storage: sessionStorage,
  });
  (window as unknown as { __mockGitHub: MockGitHub }).__mockGitHub = mock;
  const auth = createAuth({
    fetch: mock.fetch,
    // The worker's login would bounce through GitHub and land on `return`.
    navigate: (url) => {
      mock.logIn();
      const back = new URL(url, window.location.href).searchParams.get(
        "return",
      );
      window.location.assign(back ?? "/");
    },
  });
  return {
    auth,
    api: createGitHubApi({ auth, fetch: mock.fetch }),
    onboarding: {
      newRepo: () => mock.createRepo(),
      install: async () => mock.installApp(),
    },
    mock,
  };
}
