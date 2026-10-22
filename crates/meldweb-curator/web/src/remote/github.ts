/**
 * GitHub as a remote's host (ADR-0041): the login, and finding or making the
 * `mtg` repo, through the GitHub App and its worker routes as before
 * (github-login-static-site.md). Pushing and fetching are git's, over the
 * relay, with the login's token as the password; nothing here calls the API
 * for them.
 *
 * Whether git over HTTPS takes a GitHub App's *user* token (`ghu_`) is not
 * written down: GitHub documents it for the App's installation tokens
 * ("Authenticating as a GitHub App installation", docs.github.com, under
 * "HTTP-based Git access": `x-access-token` as the user, the token as the
 * password, with the Contents permission). The user token is an OAuth token
 * of that App limited to the same repo and permission, and an OAuth token
 * (`gho_`) was observed to fetch and push this way.
 */
import { createGitHubApi, type RepoRef } from "../github/api";
import { type Auth, AuthError, createAuth } from "../github/auth";
import { githubOnboarding, type Onboarding } from "../github/onboarding";
import { type FoundRepo, findMagicRepo } from "../github/repo";
import { basicToken, type Credentials } from "./transport";

export const GITHUB_HOST = "github.com";

export function isGitHub(url: string): boolean {
  try {
    return new URL(url).host === GITHUB_HOST;
  } catch {
    return false;
  }
}

export const githubUrl = (repo: RepoRef) =>
  `https://${GITHUB_HOST}/${repo.owner}/${repo.name}.git`;

export interface GitHubHost {
  credentials: Credentials;
  /** Whether a login is held or can be refreshed without leaving the page. */
  loggedIn(): Promise<boolean>;
  /** Leaves for GitHub's login, coming back to `returnPath`. */
  login(returnPath: string): void;
  /** The `mtg` repo the App is installed on, or which onboarding step is left. */
  find(): Promise<FoundRepo>;
  onboarding: Onboarding;
}

export function githubHost(auth: Auth = createAuth()): GitHubHost {
  // The worker answers no login where it is not running, as in plain
  // `pnpm dev`: that is no login, not a failure.
  const token = async () => {
    try {
      return await auth.token();
    } catch (e) {
      if (e instanceof AuthError) return null;
      throw e;
    }
  };
  const tokens = new Map<string, string>();
  const header = (t: string | null) => {
    if (!t) return null;
    const h = basicToken(t);
    tokens.set(h, t);
    return h;
  };
  return {
    credentials: {
      authorization: async () => header(await token()),
      async refreshed(stale) {
        const was = tokens.get(stale);
        if (!was) return null;
        try {
          return header(await auth.refreshed(was));
        } catch (e) {
          if (e instanceof AuthError) return null;
          throw e;
        }
      },
    },
    loggedIn: async () => (await token()) !== null,
    login: (returnPath) => auth.login(returnPath),
    find: () => findMagicRepo(createGitHubApi({ auth })),
    onboarding: githubOnboarding,
  };
}
