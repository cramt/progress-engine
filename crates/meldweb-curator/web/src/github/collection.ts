/**
 * The collection (ADR-0023): one `collection.toml` at the Magic repo's root,
 * saved the way a deck is. A repo without it owns nothing yet, and the first
 * save creates it.
 */
import type { Repo } from "../repo/api";
import { openFile } from "./repoFile";

export const COLLECTION_PATH = "collection.toml";

/** The collection's text and blob sha; `sha: null` while there is no file. */
export async function loadCollection(
  repo: Repo,
): Promise<{ text: string; sha: string | null }> {
  const file = await openFile(repo, COLLECTION_PATH);
  return file ? { text: file.text, sha: file.sha } : { text: "", sha: null };
}
