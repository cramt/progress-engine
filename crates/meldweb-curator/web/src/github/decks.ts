/**
 * The decks in the Magic repo: `decks/*.deck.toml`, each identified by its
 * path, which is slugged from the name when the deck is made and never follows
 * a rename (ADR-0021).
 */
import { ConflictError, type GitHubApi, type RepoRef } from "./api";
import type { DeckText } from "./deckText";
import { commitDeck } from "./save";

export const DECKS_DIR = "decks";
export const DECK_SUFFIX = ".deck.toml";

export interface DeckEntry {
  path: string;
  sha: string;
  /** The deck's `name`, or the file stem when it declares none. */
  name: string;
  /** Set when the file is not a deck the format allows, with why. */
  refused?: string;
}

/** `decks/lantern.deck.toml` → `lantern`. */
export function deckStem(path: string): string {
  const file = path.slice(path.lastIndexOf("/") + 1);
  return file.endsWith(DECK_SUFFIX) ? file.slice(0, -DECK_SUFFIX.length) : file;
}

/** Every `decks/*.deck.toml` with its name, sorted by name. */
export async function listDecks(
  api: GitHubApi,
  repo: RepoRef,
  deck: Pick<DeckText, "parseDeck">,
): Promise<DeckEntry[]> {
  const files = (await api.listDir(repo, DECKS_DIR)).filter(
    (e) => e.type === "file" && e.name.endsWith(DECK_SUFFIX),
  );
  const entries = await Promise.all(
    files.map(async (f): Promise<DeckEntry> => {
      const file = await api.getFile(repo, f.path);
      if (!file)
        return {
          path: f.path,
          sha: f.sha,
          name: deckStem(f.path),
          refused: "gone",
        };
      const parsed = deck.parseDeck(file.text);
      return parsed.kind === "deck"
        ? { path: f.path, sha: file.sha, name: parsed.name ?? deckStem(f.path) }
        : {
            path: f.path,
            sha: file.sha,
            name: deckStem(f.path),
            refused: parsed.message,
          };
    }),
  );
  return entries.sort((a, b) => a.name.localeCompare(b.name));
}

/**
 * A filename from a deck name: lowercase ASCII letters and digits joined by
 * single dashes, accents folded. Empty when nothing usable is left.
 */
export function slugify(name: string): string {
  return name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function deckPath(name: string): string {
  return `${DECKS_DIR}/${slugify(name)}${DECK_SUFFIX}`;
}

export type NewDeckSource =
  | { kind: "empty"; format: string }
  | { kind: "archidekt"; text: string; format?: string };

export type CreatedDeck =
  | {
      kind: "created";
      path: string;
      sha: string;
      text: string;
      /** Archidekt lines the import could not read, with why; empty otherwise. */
      unreadable: { line: number; text: string; reason: string }[];
    }
  | { kind: "refused"; message: string };

/**
 * Writes `decks/<slug>.deck.toml` as its first commit, through the same
 * `commitDeck` as every save, and refuses a slug that is already taken.
 */
export async function createDeck(
  api: GitHubApi,
  repo: RepoRef,
  name: string,
  source: NewDeckSource,
  deck: Pick<
    DeckText,
    | "parseDeck"
    | "newDeck"
    | "importArchidekt"
    | "setDeckMeta"
    | "commitMessage"
  >,
): Promise<CreatedDeck> {
  const slug = slugify(name);
  if (!slug) {
    return {
      kind: "refused",
      message: "A deck name needs a letter or a digit to make a filename from.",
    };
  }
  const path = deckPath(name);
  const taken = () => ({
    kind: "refused" as const,
    message: `${path} already exists. A deck's file never moves, so pick another name.`,
  });
  if (await api.getFile(repo, path)) return taken();

  let text: string;
  let unreadable: { line: number; text: string; reason: string }[] = [];
  if (source.kind === "empty") {
    text = deck.newDeck(name, source.format);
  } else {
    const imported = deck.importArchidekt(source.text);
    if (imported.kind === "refused") {
      return { kind: "refused", message: imported.message };
    }
    unreadable = imported.unreadable;
    text = imported.toml;
    // Archidekt's text carries no deck name, so the dialog's goes in.
    text = deck.setDeckMeta(text, name, source.format);
    const parsed = deck.parseDeck(text);
    if (parsed.kind === "refused") {
      return { kind: "refused", message: parsed.message };
    }
  }

  try {
    const { sha } = await commitDeck(api, repo, path, {
      text,
      message: deck.commitMessage("", text, path),
      sha: null,
    });
    return { kind: "created", path, sha, text, unreadable };
  } catch (e) {
    if (e instanceof ConflictError) return taken();
    throw e;
  }
}
