/**
 * The wanted list (ADR-0034): one `wanted.toml` at the Magic repo's root,
 * saved the way the collection is, and the deck files its derived half reads.
 */

import type { Repo } from "../repo/api";
import type { DeckFile } from "../wanted";
import { DECK_SUFFIX, DECKS_DIR } from "./decks";
import { openFile } from "./repoFile";

export const WANTED_PATH = "wanted.toml";

/** The list's text and blob sha; `sha: null` while there is no file. */
export async function loadWanted(
  repo: Repo,
): Promise<{ text: string; sha: string | null }> {
  const file = await openFile(repo, WANTED_PATH);
  return file ? { text: file.text, sha: file.sha } : { text: "", sha: null };
}

/** Every `decks/*.deck.toml` as its text, for Rust to read. */
export async function loadDeckFiles(repo: Repo): Promise<DeckFile[]> {
  const files = (await repo.listDir(DECKS_DIR)).filter(
    (e) => e.type === "file" && e.name.endsWith(DECK_SUFFIX),
  );
  const read = await Promise.all(
    files.map(async (f) => {
      const file = await repo.getFile(f.path);
      return file ? { path: f.path, text: file.text } : null;
    }),
  );
  return read.filter((d): d is DeckFile => d !== null);
}
