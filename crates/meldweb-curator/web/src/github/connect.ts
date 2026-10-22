/**
 * The one place the app chooses where the Magic repo is. Everything after
 * this takes the returned connection and does not ask which it got, except
 * where only one kind can answer: onboarding is GitHub's, and export and
 * import are the browser repo's.
 *
 * Plain `pnpm dev` and every build keep the repo in this browser (ADR-0041),
 * and need no login. `VITE_MOCK_GITHUB=1 pnpm dev` instead fakes GitHub's
 * login, onboarding and the worker in the page, with `rival`'s public repo to
 * trade against; `no-repo` or `no-install` start at onboarding. Its Magic
 * repo is held in memory, seeded as a version-1 repo (Lantern with six weeks
 * of history, a budget variant of it, a snapshot, and Loam) that migrates to
 * version 2 as it opens, and is seeded again on reload. The fake is on
 * `window.__mockGitHub`.
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
      /** The Magic repo, once the login and onboarding let it open. */
      repo: LocalRepo;
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
  const [{ createMockGitHub }, seed, { memoryRepo }, { author }, { loadDeck }] =
    await Promise.all([
      import("./mock"),
      import("./mockSeed"),
      import("../repo/memory"),
      import("../repo/author"),
      import("../deck"),
    ]);
  // The repo runs in this page rather than a worker, so the wasm it runs on
  // has to be here first.
  await loadDeck();
  const mock = createMockGitHub({
    start: mode === "no-repo" || mode === "no-install" ? mode : "ready",
    others: seed.others,
    storage: sessionStorage,
  });
  let clock: number | null = null;
  const repo = memoryRepo(
    () => author(),
    () => clock ?? Date.now(),
  );
  await seed.seedRepo(repo, (ms) => (clock = ms), Date.now());
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
    repo,
    others: createPublicRepos({ fetch: mock.fetch }),
    onboarding: {
      newRepo: () => mock.createRepo(),
      install: async () => mock.installApp(),
    },
    mock,
  };
}
