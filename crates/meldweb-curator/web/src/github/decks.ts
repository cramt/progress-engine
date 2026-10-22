/**
 * The decks in the Magic repo (ADR-0041): each a line of history of its own,
 * the branch `decks/<stem>/main`, whose tree holds `<stem>.deck.toml`. The
 * stem is the deck's identity, slugged from the name when the deck is made
 * and never following a rename (ADR-0021). A fork's line starts on a commit of
 * the deck it was forked from, so which deck that was is read from history.
 */
import { parseCollection, undeclarePlace } from "../collection";
import type { CardRef, SetOnly } from "../deck";
import { ConflictError, type Repo } from "../repo/api";
import { fetchPrintings, fetchPrintingsInSets, printingKey } from "../scryfall";
import { COLLECTION_PATH } from "./collection";
import type { DeckText } from "./deckText";
import { commitEdit, settled, spotKey, track } from "./repoFile";

export const DECK_SUFFIX = ".deck.toml";
export const CRITERIA_SUFFIX = ".criteria.toml";

/** Where a deck's file is: its path at the root of its line, and the branch. */
export interface DeckFile {
  path: string;
  branch: string;
}

/** `lantern` → `lantern.deck.toml` on `decks/lantern/main`. */
export function deckFile(stem: string, branch = "main"): DeckFile {
  return { path: `${stem}${DECK_SUFFIX}`, branch: `decks/${stem}/${branch}` };
}

/** `lantern` → `lantern.criteria.toml`, its Gauntlet criteria, on its line. */
export function criteriaFile(stem: string, branch = "main"): DeckFile {
  return {
    path: `${stem}${CRITERIA_SUFFIX}`,
    branch: `decks/${stem}/${branch}`,
  };
}

/** The tag prefix of a deck's snapshots: `decks/lantern/`. */
export const snapshotPrefix = (stem: string) => `decks/${stem}/`;

export interface DeckEntry {
  stem: string;
  /** The tip of its `main`, which deleting it is guarded by. */
  commit: string;
  sha: string;
  /** The deck's `name`, or its stem when it declares none. */
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
  /** The stem of the deck it was forked from, as its history says. */
  parent?: string;
  /** The deck's text on its `main`, which pages counting every deck read. */
  text: string;
}

/**
 * A deck's stem from what may name it: the stem itself, a repo-version-1
 * path (`decks/lantern.deck.toml`), or its file (`lantern.deck.toml`), so an
 * old link still opens the deck.
 */
export function deckStem(name: string): string {
  const file = name.slice(name.lastIndexOf("/") + 1);
  return file.endsWith(DECK_SUFFIX) ? file.slice(0, -DECK_SUFFIX.length) : file;
}

/** Every deck, `decks/<stem>/main`, with its name, sorted by name. */
export async function listDecks(
  repo: Repo,
  deck: Pick<DeckText, "parseDeck">,
): Promise<DeckEntry[]> {
  const lines = await repo.decks();
  const entries = lines.map((line): DeckEntry => {
    const base = {
      stem: line.stem,
      commit: line.commit,
      sha: line.file?.sha ?? "",
      text: line.file?.text ?? "",
      ...(line.parent ? { parent: line.parent } : {}),
    };
    if (!line.file) {
      return {
        ...base,
        name: line.stem,
        refused: `${line.stem}${DECK_SUFFIX} is not on its line`,
      };
    }
    const parsed = deck.parseDeck(line.file.text);
    if (parsed.kind === "deck") {
      return {
        ...base,
        name: parsed.name ?? line.stem,
        ...(parsed.format ? { format: parsed.format } : {}),
        ...(parsed.cover ? { cover: parsed.cover } : {}),
        total: parsed.total,
        commanders: parsed.cards
          .filter((c) => c.place === "commander")
          .map((c) => c.card),
      };
    }
    return { ...base, name: line.stem, refused: parsed.message };
  });
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

/** The branch a deck named `name` would be made as: `decks/<slug>/main`. */
export function deckBranchFor(name: string): string {
  return deckFile(slugify(name)).branch;
}

export type NewDeckSource =
  | { kind: "empty"; format: string }
  | { kind: "archidekt"; text: string; format?: string }
  /**
   * A fork: `text`, the deck `of` as its commit `at` left it, as a deck of
   * its own whose line starts on that commit (ADR-0041).
   */
  | { kind: "fork"; text: string; of: string; at: string };

export type CreatedDeck =
  | {
      kind: "created";
      stem: string;
      sha: string;
      text: string;
      /** Archidekt lines the import could not read, with why; empty otherwise. */
      unreadable: { line: number; text: string; reason: string }[];
    }
  | { kind: "refused"; message: string };

/**
 * Makes the deck's line, `decks/<slug>/main`, with its file as the first
 * commit (or, for a fork, the first commit after the deck it came from), and
 * refuses a slug that is already taken.
 */
export async function createDeck(
  repo: Repo,
  name: string,
  source: NewDeckSource,
  deck: ImportText & Pick<DeckText, "newDeck">,
  lookups: Lookups = scryfallLookups,
): Promise<CreatedDeck> {
  const stem = slugify(name);
  if (!stem) {
    return {
      kind: "refused",
      message:
        "A deck name needs a letter or a digit to make its name in the repo from.",
    };
  }
  const file = deckFile(stem);
  const taken = () => ({
    kind: "refused" as const,
    message: `There is a deck ${stem} already (${file.branch}). A deck's name in the repo never moves, so pick another name.`,
  });
  if ((await repo.decks()).some((d) => d.stem === stem)) return taken();

  let text: string;
  let unreadable: { line: number; text: string; reason: string }[] = [];
  if (source.kind === "empty") {
    text = deck.newDeck(name, source.format);
  } else if (source.kind === "fork") {
    text = deck.setDeckMeta(source.text, name);
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

  if (source.kind === "fork") {
    try {
      const { sha } = await track(
        spotKey(file.path, file.branch),
        repo.putFile(file.path, {
          text,
          message: `${stem}: forked from ${source.of}`,
          sha: null,
          branch: file.branch,
          from: source.at,
        }),
      );
      return { kind: "created", stem, sha, text, unreadable };
    } catch (e) {
      if (e instanceof ConflictError) return taken();
      throw e;
    }
  }
  // A deck made with the same stem meanwhile is left as it is.
  const made = await commitEdit(
    repo,
    file.path,
    (t) => (t === null ? text : null),
    file.branch,
  );
  if (made.kind === "unchanged") return taken();
  return { kind: "created", stem, sha: made.sha, text, unreadable };
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

export type DeletedDeck =
  | { kind: "deleted" }
  | { kind: "refused"; message: string };

/** Why a deck cannot be deleted, thrown from inside the collection's edit. */
class Kept extends Error {}

/**
 * Deletes the deck as the list last read it, so a deck changed since is
 * refused as a conflict rather than lost. A deck the collection keeps copies
 * in is refused, as a place that holds anything cannot be dropped (ADR-0023);
 * an empty place for it is dropped first, in its own commit, and copies put
 * there meanwhile refuse it then too.
 */
export async function deleteDeck(
  repo: Repo,
  deck: Pick<DeckEntry, "stem" | "commit">,
): Promise<DeletedDeck> {
  try {
    await commitEdit(repo, COLLECTION_PATH, (text) => {
      if (text === null) return null;
      const parsed = parseCollection(text);
      if (parsed.kind === "refused") {
        throw new Kept(
          `collection.toml must be read to see whether copies are in this deck, and it is refused: ${parsed.message}`,
        );
      }
      const place = parsed.places.find((p) => p.deck === deck.stem);
      if (!place) return null;
      const copies = parsed.cards
        .filter((c) => c.at === place.name)
        .reduce((n, c) => n + c.qty, 0);
      if (copies > 0) {
        throw new Kept(
          `The collection has ${copies} cop${copies === 1 ? "y" : "ies"} in ${place.name}, this deck's place. Move them out in the collection first.`,
        );
      }
      return undeclarePlace(text, place.name);
    });
  } catch (e) {
    if (e instanceof Kept) return { kind: "refused", message: e.message };
    throw e;
  }
  const file = deckFile(deck.stem);
  const key = spotKey(file.path, file.branch);
  await settled(key);
  try {
    await track(key, repo.deleteDeck(deck.stem, deck.commit));
  } catch (e) {
    if (e instanceof ConflictError) {
      return {
        kind: "refused",
        message: `${deck.stem} changed since the list was read. Reload and look before deleting it.`,
      };
    }
    throw e;
  }
  return { kind: "deleted" };
}
