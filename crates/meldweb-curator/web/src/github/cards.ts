/**
 * Custom cards in the Magic repo: `cards/*.card.toml`, Mirage Mockery's
 * format, with any art beside them (ADR-0039).
 */
import type { GitHubApi, RepoRef } from "./api";
import { openFile } from "./repoFile";

export const CARDS_DIR = "cards";
export const CARD_SUFFIX = ".card.toml";
const CARD = /^cards\/[^/]+\.card\.toml$/;

export function isCardPath(path: string): boolean {
  return CARD.test(path);
}

/** `cards/proofsheet-drone.card.toml` → `proofsheet-drone`. */
export function cardStem(path: string): string {
  const file = path.slice(path.lastIndexOf("/") + 1);
  return file.endsWith(CARD_SUFFIX) ? file.slice(0, -CARD_SUFFIX.length) : file;
}

/** Where a card named `name` is made: its name slugged, as a deck's is. */
export function cardPath(name: string): string {
  const slug = name
    .normalize("NFKD")
    .replace(/[̀-ͯ]/g, "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");
  return `${CARDS_DIR}/${slug || "card"}${CARD_SUFFIX}`;
}

/** A path the card file names, relative to its own directory, from the root. */
export function besideCard(cardPath: string, relative: string): string {
  const dir = cardPath.slice(0, cardPath.lastIndexOf("/") + 1);
  const parts: string[] = [];
  for (const part of `${dir}${relative}`.split("/")) {
    if (part === "..") parts.pop();
    else if (part !== "." && part !== "") parts.push(part);
  }
  return parts.join("/");
}

export interface CardFile {
  path: string;
  text: string;
  sha: string;
}

/** Every `cards/*.card.toml`, sorted by path. */
export async function listCards(
  api: GitHubApi,
  repo: RepoRef,
): Promise<CardFile[]> {
  const files = (await api.listDir(repo, CARDS_DIR)).filter(
    (e) => e.type === "file" && e.name.endsWith(CARD_SUFFIX),
  );
  const read = await Promise.all(
    files.map(async (f) => {
      const file = await openFile(api, repo, f.path);
      return file ? { path: f.path, text: file.text, sha: file.sha } : null;
    }),
  );
  return read
    .filter((c): c is CardFile => c !== null)
    .sort((a, b) => a.path.localeCompare(b.path));
}

/** What a new card file starts as. */
export function newCardText(name: string): string {
  return `name = ${JSON.stringify(name)}
cost = "{1}{U}"
type = "Creature — Homunculus"
rules = """
Flying
"""
pt = "1/1"
`;
}
