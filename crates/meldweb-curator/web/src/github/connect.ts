/**
 * The one place the app chooses where the Magic repo is. Everything after
 * this takes the returned connection and does not ask which it got, except
 * where only one kind can answer: onboarding is GitHub's, and export and
 * import are the browser repo's.
 *
 * Plain `pnpm dev` and every build keep the repo in this browser (ADR-0041),
 * and need no login. `VITE_MOCK_GITHUB=1 pnpm dev` instead fakes GitHub and
 * the worker in the page, seeded with Lantern (and six weeks of its history)
 * and Loam, and `rival`'s public repo to trade against; `no-repo` or
 * `no-install` start at onboarding. The fake lives in sessionStorage, so a
 * reload keeps its commits, and it is on `window.__mockGitHub` for
 * simulating an edit on github.com from the console.
 *
 * The browser's repo may have a remote, which pushes and fetches over git
 * (`src/remote/`); GitHub is its first host, whose login and onboarding
 * live in settings. The REST client (`createGitHubApi`) only finds the
 * `mtg` repo there.
 */
import type { Remote } from "../remote";
import type { LocalRepo } from "../repo/local";
import {
  createGitHubApi,
  createPublicRepos,
  type GitHubApi,
  type PublicRepos,
} from "./api";
import { type Auth, createAuth } from "./auth";
import type { MockGitHub } from "./mock";
import type { Onboarding } from "./onboarding";

export type Connection =
  | {
      kind: "local";
      repo: LocalRepo;
      /** Its remote, and keeping the two in step. */
      remote: Remote;
      /** Other players' public repos on GitHub, read without a login to trade. */
      others: PublicRepos;
    }
  | {
      kind: "github";
      auth: Auth;
      github: GitHubApi;
      others: PublicRepos;
      onboarding: Onboarding;
      /** Only for dev tooling; app code must not branch on it. */
      mock: MockGitHub;
    };

let connection: Promise<Connection> | null = null;

export function connect(): Promise<Connection> {
  connection ??= choose();
  return connection;
}

async function choose(): Promise<Connection> {
  const mode = import.meta.env.VITE_MOCK_GITHUB;
  if (!mode) {
    const [{ openLocalRepo }, { createRemote }] = await Promise.all([
      import("../repo/browser"),
      import("../remote"),
    ]);
    const repo = openLocalRepo();
    return {
      kind: "local",
      repo,
      remote: createRemote(repo),
      others: createPublicRepos(),
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
    kind: "github",
    auth,
    github: createGitHubApi({ auth, fetch: mock.fetch }),
    others: createPublicRepos({ fetch: mock.fetch }),
    onboarding: {
      newRepo: () => mock.createRepo(),
      install: async () => mock.installApp(),
    },
    mock,
  };
}
