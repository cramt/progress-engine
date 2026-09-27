/**
 * The two trips to github.com that make a Magic repo Curator can use. Curator
 * never creates the repo itself, so the app stays Contents-only (ADR-0021).
 */
import { AUTH_ENDPOINTS } from "./auth";
import { MAGIC_REPO } from "./repo";

/**
 * GitHub's new-repository page with the form filled in. The documented query
 * parameters are `name`, `description`, `visibility` (`public`/`private`),
 * `owner` (a login, or `@me`) and `template_owner`/`template_name`
 * (docs.github.com, "Creating a new repository from a URL query"). Visibility
 * is left to the user.
 */
export function newRepoUrl(): string {
  const q = new URLSearchParams({
    name: MAGIC_REPO,
    owner: "@me",
    description: "My Magic decks, kept by Meldweb Curator",
  });
  return `https://github.com/new?${q}`;
}

/**
 * Installing the app, where the user picks "only select repositories" and
 * `mtg`. The only documented query parameter is `state`, which is not used:
 * with authorization requested during installation GitHub returns through the
 * worker's callback, which checks its own state.
 */
export function installUrl(
  slug: string | undefined = import.meta.env.VITE_GITHUB_APP_SLUG,
): string {
  if (!slug) throw new Error("VITE_GITHUB_APP_SLUG is not set");
  return `https://github.com/apps/${encodeURIComponent(slug)}/installations/new`;
}

/**
 * The install URL as the worker knows it (`GET /api/auth/app`), falling back
 * to `VITE_GITHUB_APP_SLUG` when the worker is not there or not configured.
 */
export async function resolveInstallUrl(
  fetchImpl: typeof fetch = globalThis.fetch.bind(globalThis),
): Promise<string> {
  try {
    const r = await fetchImpl(AUTH_ENDPOINTS.app);
    if (r.ok) {
      const body = (await r.json()) as { install_url?: unknown };
      if (typeof body.install_url === "string") return body.install_url;
    }
  } catch {
    // fall back to the build's slug
  }
  return installUrl();
}

/** The onboarding steps as actions, so the mock can stand in for github.com. */
export interface Onboarding {
  /** Opens the new-repo page in a new tab; the user comes back to install. */
  newRepo(): void;
  /** Leaves for the install page, which returns through the login callback. */
  install(): Promise<void>;
}

export const githubOnboarding: Onboarding = {
  newRepo() {
    window.open(newRepoUrl(), "_blank", "noopener");
  },
  async install() {
    window.location.assign(await resolveInstallUrl());
  },
};
