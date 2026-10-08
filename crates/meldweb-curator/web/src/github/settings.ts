/**
 * `meldweb.toml` at the Magic repo's root (ADR-0026): Curator's own settings,
 * so far which printing of a card to offer first, edited at `/settings`. A
 * repo without it gets the default rules, so the file is never required.
 */
import type { FileAt, GitHubApi, RepoRef } from "./api";
import { settled } from "./repoFile";

export const SETTINGS_PATH = "meldweb.toml";

/** The settings file, or null when the repo has none. */
export async function loadSettingsFile(
  api: GitHubApi,
  repo: RepoRef,
): Promise<FileAt | null> {
  // Leaving the settings page just now may still be saving; read after it lands.
  await settled(SETTINGS_PATH);
  return api.getFile(repo, SETTINGS_PATH);
}
