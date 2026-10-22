/**
 * A deck's past, as git already keeps it (ADR-0024). Every save is a commit
 * whose message is the deck's changelog (ADR-0021), and each deck is a line of
 * history of its own (ADR-0041), so a deck's history is the commits on its
 * `main` that touched its file, and a version of it is the file at one of
 * them. Any other branch of the deck has its own line the same way. A
 * snapshot is an annotated tag on such a commit, named under the deck's stem
 * so each deck lists only its own.
 */
import type { FileAt, Repo, Revision, Snapshot } from "../repo/api";
import { deckFile, slugify, snapshotPrefix } from "./decks";
import { settled, spotKey } from "./repoFile";

export type { Revision, Snapshot };
export { snapshotPrefix };

/** Revisions a page of the timeline asks for. */
export const PAGE = 30;

/**
 * The commits on the deck's branch (`main` by default) that touched its file,
 * newest first, `PAGE` at a time.
 */
export function revisions(
  repo: Repo,
  stem: string,
  page = 1,
  on = "main",
): Promise<Revision[]> {
  const { path, branch } = deckFile(stem, on);
  return repo.history(path, { page, perPage: PAGE, branch });
}

/** The newest commit to the deck made no later than `when`: the deck as it was then. */
export async function revisionAt(
  repo: Repo,
  stem: string,
  when: Date,
  on = "main",
): Promise<Revision | null> {
  const { path, branch } = deckFile(stem, on);
  const [hit] = await repo.history(path, {
    until: when.toISOString(),
    perPage: 1,
    branch,
  });
  return hit ?? null;
}

/**
 * The last commit to the deck's branch once every pending save has landed:
 * what "now" is in the repo, to snapshot, fork or branch from.
 */
export async function head(
  repo: Repo,
  stem: string,
  on = "main",
): Promise<Revision | null> {
  const { path, branch } = deckFile(stem, on);
  await settled(spotKey(path, branch));
  const [last] = await repo.history(path, { perPage: 1, branch });
  return last ?? null;
}

// A commit never changes, so the file at one is read once per page load.
const versions = new WeakMap<Repo, Map<string, Promise<FileAt | null>>>();

/** The deck as commit `commit` left it, or `null` if it was not there. */
export function fileAt(
  repo: Repo,
  stem: string,
  commit: string,
): Promise<FileAt | null> {
  const { path } = deckFile(stem);
  const key = `${commit}:${path}`;
  let known = versions.get(repo);
  if (!known) {
    known = new Map();
    versions.set(repo, known);
  }
  const read = known;
  let file = read.get(key);
  if (!file) {
    file = repo.getFile(path, commit);
    read.set(key, file);
    // A failed read is not a fact about the commit; let the next ask retry.
    file.catch(() => read.delete(key));
  }
  return file;
}

/**
 * The tag a snapshot called `label` gets: the deck's prefix and the label
 * slugged, which git accepts as a ref name. A label with nothing to slug is
 * named for the day.
 */
export function snapshotTag(stem: string, label: string, now: Date): string {
  const slug = slugify(label) || now.toISOString().slice(0, 10);
  return `${snapshotPrefix(stem)}${slug}`;
}

/** The deck's snapshots, newest first. */
export async function deckSnapshots(
  repo: Repo,
  stem: string,
): Promise<Snapshot[]> {
  const all = await repo.snapshots(snapshotPrefix(stem));
  return all.sort((a, b) => b.date.localeCompare(a.date));
}

export type TakenSnapshot =
  | { kind: "taken"; snapshot: Snapshot }
  | { kind: "refused"; message: string };

/** Tags `commit`, on the deck's line, as the snapshot `label` of the deck. */
export async function takeSnapshot(
  repo: Repo,
  stem: string,
  label: string,
  commit: string,
  now = new Date(),
): Promise<TakenSnapshot> {
  const trimmed = label.trim();
  if (!trimmed) return { kind: "refused", message: "A snapshot needs a name." };
  const tag = snapshotTag(stem, trimmed, now);
  try {
    return {
      kind: "taken",
      snapshot: await repo.takeSnapshot({ tag, label: trimmed, commit }),
    };
  } catch (e) {
    if (e instanceof Error && e.name === "ConflictError") {
      return {
        kind: "refused",
        message: `This deck already has a snapshot named ${tag.slice(snapshotPrefix(stem).length)}.`,
      };
    }
    throw e;
  }
}
