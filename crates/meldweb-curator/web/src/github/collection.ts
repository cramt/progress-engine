/**
 * The collection (ADR-0023): one `collection.toml` at the Magic repo's root,
 * saved the way a deck is. A repo without it owns nothing yet, and the first
 * save creates it.
 */
import type { GitHubApi, RepoRef } from "./api";

export const COLLECTION_PATH = "collection.toml";

/** The collection's text and blob sha; `sha: null` while there is no file. */
export async function loadCollection(
  api: GitHubApi,
  repo: RepoRef,
): Promise<{ text: string; sha: string | null }> {
  const file = await api.getFile(repo, COLLECTION_PATH);
  return file ? { text: file.text, sha: file.sha } : { text: "", sha: null };
}
