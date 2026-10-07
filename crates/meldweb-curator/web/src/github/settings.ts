/**
 * `meldweb.toml` at the Magic repo's root (ADR-0026): Curator's own settings,
 * so far which printing of a card to offer first. A repo without it gets the
 * default rules, so the file is never required.
 */
import type { GitHubApi, RepoRef } from "./api";

export const SETTINGS_PATH = "meldweb.toml";

/** The settings' text, or null when the repo has no `meldweb.toml`. */
export async function loadSettings(
  api: GitHubApi,
  repo: RepoRef,
): Promise<string | null> {
  const file = await api.getFile(repo, SETTINGS_PATH);
  return file ? file.text : null;
}
