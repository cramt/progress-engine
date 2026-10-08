/**
 * The decks in the Magic repo: `decks/*.deck.toml`, each identified by its
 * path, which is slugged from the name when the deck is made and never follows
 * a rename (ADR-0021).
 */
import {
  collectionCommitMessage,
  parseCollection,
  undeclarePlace,
} from "../collection";
import type { CardRef, SetOnly } from "../deck";
import { fetchPrintings, fetchPrintingsInSets, printingKey } from "../scryfall";
import { ConflictError, type GitHubApi, type RepoRef } from "./api";
import { COLLECTION_PATH, loadCollection } from "./collection";
import type { DeckText } from "./deckText";
import { settled } from "./repoFile";
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
  /** The printing it chose to stand for it, ahead of its commanders. */
  cover?: { set: string; num: string };
  /** The path of the deck it is a variant of. */
  variantOf?: string;
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
          ...(parsed.cover ? { cover: parsed.cover } : {}),
          ...(parsed.variantOf ? { variantOf: parsed.variantOf } : {}),
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
  | { kind: "archidekt"; text: string; format?: string }
  /** A copy of `text`, a version of the deck at `of`, as a variant of it. */
  | { kind: "variant"; text: string; of: string };

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
  deck: ImportText &
    Pick<DeckText, "newDeck" | "commitMessage" | "setVariantOf">,
  lookups: Lookups = scryfallLookups,
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
  } else if (source.kind === "variant") {
    text = deck.setVariantOf(deck.setDeckMeta(source.text, name), source.of);
  } else {
    const made = await deckFromArchidekt(
      source.text,
      name,
      source.format,
      deck,
      lookups,
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

type ImportText = Pick<
  DeckText,
  | "parseDeck"
  | "importArchidekt"
  | "setCardPrinting"
  | "setDeckMeta"
  | "declareCategory"
  | "setCardCategories"
>;

/** What an import asks Scryfall, as one seam tests can replace. */
export interface Lookups {
  inSets: typeof fetchPrintingsInSets;
  printings: typeof fetchPrintings;
}

const scryfallLookups: Lookups = {
  inSets: fetchPrintingsInSets,
  printings: fetchPrintings,
};

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
  deck: ImportText,
  lookups: Lookups = scryfallLookups,
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
    lookups.inSets,
    unreadable,
  );
  text = await fileByType(text, deck, lookups.printings);
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

// Archidekt's own order when a card has several types: a land creature is a
// land, an artifact creature a creature.
const TYPES = [
  "Land",
  "Creature",
  "Battle",
  "Planeswalker",
  "Instant",
  "Sorcery",
  "Artifact",
  "Enchantment",
] as const;

function mainType(typeLine: string): string | undefined {
  const types = (typeLine.split(" — ")[0] ?? "").split(" ");
  return TYPES.find((t) => types.includes(t));
}

/**
 * Files every card the text left without a category under its front face's
 * main type. Archidekt gives a bracketless line a category of its own choosing
 * as it imports it (archidekt-import-shapes.md, rule 6), which for its own
 * export means the type it was grouped by. A card Scryfall cannot place is
 * left as it is.
 */
async function fileByType(
  text: string,
  deck: Pick<DeckText, "parseDeck" | "declareCategory" | "setCardCategories">,
  printings: typeof fetchPrintings,
): Promise<string> {
  const parsed = deck.parseDeck(text);
  if (parsed.kind === "refused") return text;
  const bare = parsed.cards.filter((c) => c.categories.length === 0);
  if (bare.length === 0) return text;
  const found = await printings(bare).catch(() => new Map());
  for (const card of bare) {
    const typeLine = found.get(printingKey(card.card))?.typeLine;
    const type = typeLine && mainType(typeLine);
    if (!type) continue;
    text = deck.declareCategory(text, type);
    text = deck.setCardCategories(text, card.index, [type]);
  }
  return text;
}

/**
 * One commit of `edit` applied to the deck as it is on GitHub now, after any
 * save still landing from its editor: what the deck list's Rename and Set
 * cover do without opening the deck.
 */
export async function editDeckFile(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  edit: (text: string) => string,
  deck: Pick<DeckText, "commitMessage">,
): Promise<void> {
  await settled(path);
  const file = await api.getFile(repo, path);
  if (!file) throw new Error(`${path} is no longer in the repo`);
  const text = edit(file.text);
  if (text === file.text) return;
  await commitDeck(api, repo, path, {
    text,
    message: deck.commitMessage(file.text, text, path),
    sha: file.sha,
  });
}

export type DeletedDeck =
  | { kind: "deleted" }
  | { kind: "refused"; message: string };

/**
 * Deletes the deck as the list last read it, so a deck changed since is
 * refused as a conflict rather than lost. A deck the collection keeps copies
 * in is refused, as a place that holds anything cannot be dropped (ADR-0023);
 * an empty place for it is dropped first, in its own commit.
 */
export async function deleteDeck(
  api: GitHubApi,
  repo: RepoRef,
  deck: Pick<DeckEntry, "path" | "sha">,
): Promise<DeletedDeck> {
  await Promise.all([settled(deck.path), settled(COLLECTION_PATH)]);
  const collection = await loadCollection(api, repo);
  if (collection.sha !== null) {
    const parsed = parseCollection(collection.text);
    if (parsed.kind === "refused") {
      return {
        kind: "refused",
        message: `collection.toml must be read to see whether copies are in this deck, and it is refused: ${parsed.message}`,
      };
    }
    const place = parsed.places.find((p) => p.deck === deck.path);
    if (place) {
      const copies = parsed.cards
        .filter((c) => c.at === place.name)
        .reduce((n, c) => n + c.qty, 0);
      if (copies > 0) {
        return {
          kind: "refused",
          message: `The collection has ${copies} cop${copies === 1 ? "y" : "ies"} in ${place.name}, this deck's place. Move them out in the collection first.`,
        };
      }
      const text = undeclarePlace(collection.text, place.name);
      await api.putFile(repo, COLLECTION_PATH, {
        text,
        message: collectionCommitMessage(
          collection.text,
          text,
          COLLECTION_PATH,
        ),
        sha: collection.sha,
      });
    }
  }
  try {
    await api.deleteFile(repo, deck.path, {
      message: `${deckStem(deck.path)}: delete`,
      sha: deck.sha,
    });
  } catch (e) {
    if (e instanceof ConflictError) {
      return {
        kind: "refused",
        message: `${deck.path} changed on GitHub since the list was read. Reload and look before deleting it.`,
      };
    }
    throw e;
  }
  return { kind: "deleted" };
}
