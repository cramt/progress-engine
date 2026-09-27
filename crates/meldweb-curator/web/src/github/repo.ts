/**
 * Finding the user's Magic repo and opening it at a repo version Curator
 * knows (ADR-0021).
 */
import { ConflictError, type GitHubApi, type RepoRef } from "./api";

/** The Magic repo's fixed name. */
export const MAGIC_REPO = "mtg";

export type FoundRepo =
  | { kind: "found"; login: string; repo: RepoRef }
  /**
   * The app cannot reach `<login>/mtg`. `repoExists` says whether GitHub
   * shows the repo at all; a private repo the app is not installed on reads
   * as not existing, so onboarding offers both steps either way.
   */
  | { kind: "onboarding"; login: string; repoExists: boolean };

/**
 * `<user>/mtg` when the app is installed on it, found through the user's
 * installations as github-login-static-site.md says, rather than trusting any
 * installation id from a redirect.
 */
export async function findMagicRepo(api: GitHubApi): Promise<FoundRepo> {
  const { login } = await api.user();
  const matches = (r: RepoRef) =>
    r.owner.toLowerCase() === login.toLowerCase() &&
    r.name.toLowerCase() === MAGIC_REPO;
  for (const installation of await api.installations()) {
    const found = (await api.installationRepos(installation.id)).find(matches);
    if (found) return { kind: "found", login, repo: found };
  }
  const repoExists = await api.repoExists({ owner: login, name: MAGIC_REPO });
  return { kind: "onboarding", login, repoExists };
}

/** The repo version this Curator reads and writes. */
export const KNOWN_VERSION = 1;

export const VERSION_PATH = "VERSION";

/**
 * Takes a repo from `from` to `from + 1`, in one commit that also rewrites
 * `VERSION`. None exist yet: version 1 is the first.
 */
export interface Migration {
  from: number;
  migrate(api: GitHubApi, repo: RepoRef): Promise<void>;
}

export const MIGRATIONS: readonly Migration[] = [];

export type OpenedRepo =
  | { kind: "open"; version: number }
  | { kind: "refused"; message: string };

export interface OpenOptions {
  known?: number;
  migrations?: readonly Migration[];
}

function parseVersion(text: string): number | null {
  const t = text.trim();
  return /^\d+$/.test(t) ? Number(t) : null;
}

/**
 * Reads `VERSION`: missing writes `1`, newer than Curator knows refuses, older
 * runs the migrations in order before anything is edited.
 */
export async function openMagicRepo(
  api: GitHubApi,
  repo: RepoRef,
  options: OpenOptions = {},
): Promise<OpenedRepo> {
  const known = options.known ?? KNOWN_VERSION;
  const migrations = options.migrations ?? MIGRATIONS;

  let file = await api.getFile(repo, VERSION_PATH);
  if (!file) {
    try {
      await api.putFile(repo, VERSION_PATH, {
        text: "1\n",
        message: "curator: this repo is at version 1",
        sha: null,
      });
    } catch (e) {
      // Another tab wrote it first; read what it wrote.
      if (!(e instanceof ConflictError)) throw e;
    }
    file = await api.getFile(repo, VERSION_PATH);
    if (!file)
      return { kind: "refused", message: "VERSION could not be written" };
  }

  let version = parseVersion(file.text);
  if (version === null) {
    return {
      kind: "refused",
      message: `VERSION holds ${JSON.stringify(file.text.trim())}, not a repo version`,
    };
  }
  if (version > known) {
    return {
      kind: "refused",
      message: `This repo is at version ${version}, and this Curator only knows up to version ${known}. Reload to get a newer Curator; nothing was changed.`,
    };
  }
  while (version < known) {
    const step = migrations.find((m) => m.from === version);
    if (!step) {
      return {
        kind: "refused",
        message: `This repo is at version ${version}, and Curator has no way from it to version ${known}.`,
      };
    }
    await step.migrate(api, repo);
    const after = await api.getFile(repo, VERSION_PATH);
    const next = after && parseVersion(after.text);
    if (next !== version + 1) {
      return {
        kind: "refused",
        message: `Migrating from version ${version} did not leave the repo at ${version + 1}.`,
      };
    }
    version = next;
  }
  return { kind: "open", version };
}
