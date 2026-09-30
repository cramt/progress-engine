/**
 * The decks in the Magic repo: `decks/*.deck.toml`, each identified by its
 * path, which is slugged from the name when the deck is made and never follows
 * a rename (ADR-0021).
 */
import type { CardRef, SetOnly } from "../deck";
import { fetchPrintingsInSets } from "../scryfall";
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
  /** The deck's `format`, when it declares one. */
  format?: string;
  /** Cards in the deck, as the editor counts them. */
  total?: number;
  /** The cards in its commander-typed categories, for the deck list's art. */
  commanders?: CardRef[];
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
      if (parsed.kind === "deck") {
        return {
          path: f.path,
          sha: file.sha,
          name: parsed.name ?? deckStem(f.path),
          ...(parsed.format ? { format: parsed.format } : {}),
          total: parsed.total,
          commanders: parsed.cards
            .filter((c) => c.place === "commander")
            .map((c) => c.card),
        };
      }
      return {
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
    | "setCardPrinting"
    | "setDeckMeta"
    | "commitMessage"
  >,
  inSets: typeof fetchPrintingsInSets = fetchPrintingsInSets,
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
    const made = await deckFromArchidekt(
      source.text,
      name,
      source.format,
      deck,
      inSets,
    );
    if (made.kind === "refused") return made;
    ({ text, unreadable } = made);
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

export type FromArchidekt =
  | {
      kind: "deck";
      text: string;
      /** Archidekt lines the import could not read, with why; empty otherwise. */
      unreadable: { line: number; text: string; reason: string }[];
    }
  | { kind: "refused"; message: string };

/**
 * Archidekt text as a whole `.deck.toml` named `name`: what New deck commits,
 * and what Replace from Archidekt swaps an open deck's text for.
 */
export async function deckFromArchidekt(
  source: string,
  name: string,
  format: string | undefined,
  deck: Pick<
    DeckText,
    "parseDeck" | "importArchidekt" | "setCardPrinting" | "setDeckMeta"
  >,
  inSets: typeof fetchPrintingsInSets = fetchPrintingsInSets,
): Promise<FromArchidekt> {
  const imported = deck.importArchidekt(source);
  if (imported.kind === "refused") {
    return { kind: "refused", message: imported.message };
  }
  const unreadable = [...imported.unreadable];
  let text = await pinSets(
    imported.toml,
    imported.setOnly,
    deck,
    inSets,
    unreadable,
  );
  // Archidekt's text carries no deck name, so the caller's goes in.
  text = deck.setDeckMeta(text, name, format);
  const parsed = deck.parseDeck(text);
  if (parsed.kind === "refused") {
    return { kind: "refused", message: parsed.message };
  }
  return { kind: "deck", text, unreadable };
}

/**
 * Names each card whose Archidekt line gave a set and no number by the
 * printing Scryfall has in that set, and says of each it could not which one
 * stayed named by name, so a set the import dropped is never silent.
 */
async function pinSets(
  text: string,
  setOnly: readonly SetOnly[],
  deck: Pick<DeckText, "setCardPrinting">,
  inSets: typeof fetchPrintingsInSets,
  unreadable: { line: number; text: string; reason: string }[],
): Promise<string> {
  if (setOnly.length === 0) return text;
  let found: ({ set: string; num: string } | null)[];
  let why: string;
  try {
    found = await inSets(setOnly);
    why = "Scryfall has no card of that name in it";
  } catch (e) {
    found = setOnly.map(() => null);
    why = `Scryfall could not be asked (${e instanceof Error ? e.message : String(e)})`;
  }
  setOnly.forEach((s, i) => {
    const printing = found[i];
    if (printing) {
      text = deck.setCardPrinting(text, s.index, printing.set, printing.num);
    } else {
      unreadable.push({
        line: s.line,
        text: s.text,
        reason: `kept by name without its set (${s.set}): ${why}`,
      });
    }
  });
  unreadable.sort((a, b) => a.line - b.line);
  return text;
}
