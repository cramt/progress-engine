/**
 * A deck's past, as git already keeps it (ADR-0024). Every save is a commit
 * whose message is the deck's changelog (ADR-0021), so a deck's history is
 * the commits that touched its path, and a version of it is the file at one
 * of them. A snapshot is an annotated tag on such a commit, named under the
 * deck's path so each deck lists only its own.
 */
import type { FileAt, GitHubApi, RepoRef, Revision, Snapshot } from "./api";
import { DECK_SUFFIX, slugify } from "./decks";
import { settled } from "./save";

export type { Revision, Snapshot };

/** Revisions a page of the timeline asks for. */
export const PAGE = 30;

/** The commits that touched `path`, newest first, `PAGE` at a time. */
export function revisions(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  page = 1,
): Promise<Revision[]> {
  return api.history(repo, path, { page, perPage: PAGE });
}

/** The newest commit to `path` made no later than `when`: the deck as it was then. */
export async function revisionAt(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  when: Date,
): Promise<Revision | null> {
  const [hit] = await api.history(repo, path, {
    until: when.toISOString(),
    perPage: 1,
  });
  return hit ?? null;
}

/**
 * The last commit to `path` once every pending save has landed: what "now"
 * is on GitHub, to snapshot or branch from.
 */
export async function head(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
): Promise<Revision | null> {
  await settled(path);
  const [last] = await api.history(repo, path, { perPage: 1 });
  return last ?? null;
}

// A commit never changes, so the file at one is read once per page load.
const versions = new Map<string, Promise<FileAt | null>>();

/** The file at `path` as commit `commit` left it, or `null` if it was not there. */
export function fileAt(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  commit: string,
): Promise<FileAt | null> {
  const key = `${repo.owner}/${repo.name}@${commit}:${path}`;
  let file = versions.get(key);
  if (!file) {
    file = api.getFile(repo, path, commit);
    versions.set(key, file);
    // A failed read is not a fact about the commit; let the next ask retry.
    file.catch(() => versions.delete(key));
  }
  return file;
}

/** `decks/lantern.deck.toml` → `decks/lantern/`, the tag prefix of its snapshots. */
export function snapshotPrefix(path: string): string {
  const stem = path.endsWith(DECK_SUFFIX)
    ? path.slice(0, -DECK_SUFFIX.length)
    : path;
  return `${stem}/`;
}

/**
 * The tag a snapshot called `label` gets: the deck's prefix and the label
 * slugged, which git accepts as a ref name. A label with nothing to slug is
 * named for the day.
 */
export function snapshotTag(path: string, label: string, now: Date): string {
  const slug = slugify(label) || now.toISOString().slice(0, 10);
  return `${snapshotPrefix(path)}${slug}`;
}

/** The deck's snapshots, newest first. */
export async function deckSnapshots(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
): Promise<Snapshot[]> {
  const all = await api.snapshots(repo, snapshotPrefix(path));
  return all.sort((a, b) => b.date.localeCompare(a.date));
}

export type TakenSnapshot =
  | { kind: "taken"; snapshot: Snapshot }
  | { kind: "refused"; message: string };

/** Tags `commit` as the snapshot `label` of the deck at `path`. */
export async function takeSnapshot(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  label: string,
  commit: string,
  now = new Date(),
): Promise<TakenSnapshot> {
  const trimmed = label.trim();
  if (!trimmed) return { kind: "refused", message: "A snapshot needs a name." };
  const tag = snapshotTag(path, trimmed, now);
  try {
    return {
      kind: "taken",
      snapshot: await api.takeSnapshot(repo, { tag, label: trimmed, commit }),
    };
  } catch (e) {
    if (e instanceof Error && e.name === "ConflictError") {
      return {
        kind: "refused",
        message: `This deck already has a snapshot named ${tag.slice(snapshotPrefix(path).length)}.`,
      };
    }
    throw e;
  }
}
