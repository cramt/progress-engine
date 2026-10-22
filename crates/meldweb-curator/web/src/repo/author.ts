/**
 * Who this browser's commits are by (ADR-0041). Offline there is no login to
 * name anyone, so the name and email are set once per browser, in settings,
 * and kept outside the repo, as git keeps `user.name` in a config rather
 * than in a commit's tree.
 */

export interface Author {
  name: string;
  email: string;
}

/** Until one is set: plainly not a person, so a log shows it was never set. */
export const DEFAULT_AUTHOR: Author = {
  name: "Progress Engine",
  email: "curator@localhost",
};

const KEY = "meldweb.author";

type Storage = Pick<globalThis.Storage, "getItem" | "setItem" | "removeItem">;

const local = (): Storage | null =>
  typeof localStorage === "undefined" ? null : localStorage;

/** The author set in this browser, or `null` when none is. */
export function savedAuthor(storage: Storage | null = local()): Author | null {
  const raw = storage?.getItem(KEY);
  if (!raw) return null;
  try {
    const a = JSON.parse(raw) as Partial<Author>;
    return typeof a.name === "string" && typeof a.email === "string"
      ? { name: a.name, email: a.email }
      : null;
  } catch {
    return null;
  }
}

export function author(storage: Storage | null = local()): Author {
  return savedAuthor(storage) ?? DEFAULT_AUTHOR;
}

/** Keeps `a` for this browser's commits from now on; `null` forgets it. */
export function setAuthor(a: Author | null, storage: Storage | null = local()) {
  if (a === null) storage?.removeItem(KEY);
  else storage?.setItem(KEY, JSON.stringify(a));
}
