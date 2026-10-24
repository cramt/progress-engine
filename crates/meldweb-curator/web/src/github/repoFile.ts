/**
 * How a file reaches the Magic repo. Every write goes through here: an
 * editor's saves (ADR-0021), and the one-shot edits of a heart, a dragged
 * deck, a rename or a delete. Writes to one path are taken in order, so
 * whatever reads or writes a file next starts from what the last write left.
 */
import { ConflictError, type FileAt, type Repo } from "../repo/api";

/**
 * What names a file across writes: its path on `main`, or its branch and
 * path on a deck's line, `decks/lantern/main:lantern.deck.toml`.
 */
export function spotKey(path: string, branch?: string): string {
  return branch === undefined ? path : `${branch}:${path}`;
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
  repo: Repo,
  path: string,
  branch?: string,
): Promise<FileAt | null> {
  await settled(spotKey(path, branch));
  return repo.getFile(path, branch);
}

export type Committed =
  | { kind: "committed"; text: string; sha: string }
  /** `edit` left the file as it was, so nothing was written. */
  | { kind: "unchanged"; text: string | null; sha: string | null };

/**
 * How many times a one-shot edit is made before a conflict is given up on.
 * The edit is the page's function of the file's text, what to do rather
 * than a text, so a file another tab or a sync moved between the read and
 * the save is read and edited again, not merged into.
 */
const ATTEMPTS = 3;

/**
 * One commit of `edit`, made on `path` as the repo has it once every earlier
 * write to it has landed: a heart, a deck dragged, a rename. `edit` gets the
 * file's text, null while there is none, and returns the new text, or null to
 * write nothing. When the file moved before the commit landed, `edit` is
 * made again on what is there now, so it has to say what to do rather than
 * carry a result worked out from an older text. After the last attempt a
 * `ConflictError` is thrown; an `edit` that throws, or whose text the repo
 * refuses, writes nothing.
 */
export function commitEdit(
  repo: Repo,
  path: string,
  edit: (text: string | null) => string | null,
  branch?: string,
): Promise<Committed> {
  const key = spotKey(path, branch);
  const earlier = settled(key);
  const work = (async (): Promise<Committed> => {
    await earlier;
    for (let attempt = 1; ; attempt++) {
      const file = await repo.getFile(path, branch);
      const was = file?.text ?? null;
      const text = edit(was);
      if (text === null || text === was) {
        return { kind: "unchanged", text: was, sha: file?.sha ?? null };
      }
      const saved = await repo.save({
        path,
        ...(branch === undefined ? {} : { branch }),
        text,
        made: {
          kind: "from",
          ...(was === null ? {} : { base: was }),
          moved: { kind: "refuse" },
        },
      });
      switch (saved.kind) {
        case "committed":
          return { kind: "committed", text, sha: saved.sha };
        case "unchanged":
          return { kind: "unchanged", text, sha: saved.sha };
        case "refused":
          throw new Error(saved.message);
        case "clashed":
          throw new Error(`${path} merged, which a one-shot edit never asks`);
        case "moved":
          if (attempt >= ATTEMPTS) throw new ConflictError(saved.message);
      }
    }
  })();
  return track(key, work);
}
