/**
 * The wanted list (ADR-0034): one `wanted.toml` at the Magic repo's root,
 * saved the way the collection is, and the deck files its derived half reads.
 */
import type { DeckFile } from "../wanted";
import type { GitHubApi, RepoRef } from "./api";
import { DECK_SUFFIX, DECKS_DIR } from "./decks";
import { openFile } from "./repoFile";

export const WANTED_PATH = "wanted.toml";

/** The list's text and blob sha; `sha: null` while there is no file. */
export async function loadWanted(
  api: GitHubApi,
  repo: RepoRef,
): Promise<{ text: string; sha: string | null }> {
  const file = await openFile(api, repo, WANTED_PATH);
  return file ? { text: file.text, sha: file.sha } : { text: "", sha: null };
}

/** Every `decks/*.deck.toml` as its text, for Rust to read. */
export async function loadDeckFiles(
  api: GitHubApi,
  repo: RepoRef,
): Promise<DeckFile[]> {
  const files = (await api.listDir(repo, DECKS_DIR)).filter(
    (e) => e.type === "file" && e.name.endsWith(DECK_SUFFIX),
  );
  const read = await Promise.all(
    files.map(async (f) => {
      const file = await api.getFile(repo, f.path);
      return file ? { path: f.path, text: file.text } : null;
    }),
  );
  return read.filter((d): d is DeckFile => d !== null);
}
