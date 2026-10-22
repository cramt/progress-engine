/**
 * The wanted list (ADR-0034): one `wanted.toml` at the Magic repo's root,
 * saved the way the collection is, and the decks its derived half reads.
 */

import type { Repo } from "../repo/api";
import type { DeckFile } from "../wanted";
import { openFile } from "./repoFile";

export const WANTED_PATH = "wanted.toml";

/** The list's text and blob sha; `sha: null` while there is no file. */
export async function loadWanted(
  repo: Repo,
): Promise<{ text: string; sha: string | null }> {
  const file = await openFile(repo, WANTED_PATH);
  return file ? { text: file.text, sha: file.sha } : { text: "", sha: null };
}

/**
 * Every deck as its `main` has it (ADR-0041: only a deck's `main` is the
 * built deck), with the deck it was forked from, for Rust to read.
 */
export async function loadDeckFiles(repo: Repo): Promise<DeckFile[]> {
  return (await repo.decks()).flatMap((d) =>
    d.file
      ? [
          {
            stem: d.stem,
            text: d.file.text,
            ...(d.parent ? { parent: d.parent } : {}),
          },
        ]
      : [],
  );
}
