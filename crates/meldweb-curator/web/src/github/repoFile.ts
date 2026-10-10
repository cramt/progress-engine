/**
 * How a file reaches the Magic repo. Every write goes through here: an
 * editor's saves (ADR-0021), and the one-shot edits of a heart, a dragged
 * deck, a rename or a delete. Writes to one path are taken in order, so
 * whatever reads or writes a file next starts from what the last write left.
 */
import { collectionCommitMessage } from "../collection";
import { commitMessage } from "../deck";
import { settingsCommitMessage } from "../settings/rules";
import { wantedCommitMessage } from "../wanted";
import {
  ConflictError,
  type FileAt,
  type GitHubApi,
  type RepoRef,
} from "./api";
import { COLLECTION_PATH } from "./collection";
import { SETTINGS_PATH } from "./settings";
import { WANTED_PATH } from "./wanted";

const DECK = /^decks\/[^/]+\.deck\.toml$/;

/**
 * The changelog subject and body for one write of `path`, from Rust's diff of
 * the file kind it is. Throws for a path Curator does not write by edit.
 */
export function commitMessageFor(
  path: string,
  before: string,
  after: string,
): string {
  if (path === COLLECTION_PATH)
    return collectionCommitMessage(before, after, path);
  if (path === SETTINGS_PATH) return settingsCommitMessage(before, after);
  if (path === WANTED_PATH) return wantedCommitMessage(before, after, path);
  if (DECK.test(path)) return commitMessage(before, after, path);
  throw new Error(`${path} is not a file Curator edits`);
}

/** What is still being written to each path, as one promise per path. */
const writing = new Map<string, Promise<void>>();

/**
 * Keeps `settled(path)` pending until `work` has finished, landed or not, and
 * after every write tracked before it.
 */
export function track<T>(path: string, work: Promise<T>): Promise<T> {
  const done: Promise<void> = Promise.allSettled([settled(path), work]).then(
    () => {
      if (writing.get(path) === done) writing.delete(path);
    },
  );
  writing.set(path, done);
  return work;
}

/** Resolves once every write to `path` asked for so far has finished. */
export function settled(path: string): Promise<void> {
  return writing.get(path) ?? Promise.resolve();
}

/**
 * The file as GitHub has it once every write to it so far has landed, so an
 * editor opened right after leaving one starts from the new sha. Null when
 * the repo has no such file.
 */
export async function openFile(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
): Promise<FileAt | null> {
  await settled(path);
  return api.getFile(repo, path);
}

export type Committed =
  | { kind: "committed"; text: string; sha: string }
  /** `edit` left the file as it was, so nothing was written. */
  | { kind: "unchanged"; text: string | null; sha: string | null };

/** How many times a one-shot edit is made before a conflict is given up on. */
const ATTEMPTS = 3;

/**
 * One commit of `edit`, made on `path` as GitHub has it once every earlier
 * write to it has landed: a heart, a deck dragged, a rename. `edit` gets the
 * file's text, null while there is none, and returns the new text, or null to
 * write nothing. When the file moved on GitHub before the commit landed,
 * `edit` is made again on what is there now, so it has to say what to do
 * rather than carry a result worked out from an older text. After the last
 * attempt the `ConflictError` is thrown; an `edit` that throws writes nothing.
 */
export function commitEdit(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  edit: (text: string | null) => string | null,
): Promise<Committed> {
  const earlier = settled(path);
  const work = (async (): Promise<Committed> => {
    await earlier;
    for (let attempt = 1; ; attempt++) {
      const file = await api.getFile(repo, path);
      const was = file?.text ?? null;
      const text = edit(was);
      if (text === null || text === was) {
        return { kind: "unchanged", text: was, sha: file?.sha ?? null };
      }
      try {
        const { sha } = await api.putFile(repo, path, {
          text,
          message: commitMessageFor(path, was ?? "", text),
          sha: file?.sha ?? null,
        });
        return { kind: "committed", text, sha };
      } catch (e) {
        if (!(e instanceof ConflictError) || attempt >= ATTEMPTS) throw e;
      }
    }
  })();
  return track(path, work);
}
