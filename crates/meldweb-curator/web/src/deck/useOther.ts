import { useEffect, useState } from "react";
import { type Parsed, parseDeck } from "../deck";
import type { GitHubApi, RepoRef } from "../github/api";
import { deckStem } from "../github/decks";
import { fileAt, type Revision } from "../github/history";
import { fetchPrintings, type Printings } from "../scryfall";
import type { Viewing } from "./versions";

export type ParsedDeck = Extract<Parsed, { kind: "deck" }>;

/** The version the page shows beside the deck as it is now, as it loads. */
export type Other =
  | { kind: "loading" }
  | { kind: "missing"; message: string }
  | {
      kind: "loaded";
      text: string;
      deck: ParsedDeck;
      printings: Printings;
      /** What it is called: the deck's name then, or the other deck's. */
      name: string;
      /** The commit, for a past version. */
      revision: Revision | null;
    };

async function load(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  viewing: Exclude<Viewing, { kind: "now" }>,
): Promise<Other> {
  const [file, revision] =
    viewing.kind === "revision"
      ? await Promise.all([
          fileAt(api, repo, path, viewing.commit),
          api.revision(repo, viewing.commit),
        ])
      : [await api.getFile(repo, viewing.path), null];
  if (!file) {
    return {
      kind: "missing",
      message:
        viewing.kind === "revision"
          ? "The deck was not in the repo at that commit."
          : `There is no ${viewing.path} in the Magic repo.`,
    };
  }
  const parsed = parseDeck(file.text);
  if (parsed.kind === "refused") {
    return {
      kind: "missing",
      message: `That version is not a deck Curator can read: ${parsed.message}`,
    };
  }
  return {
    kind: "loaded",
    text: file.text,
    deck: parsed,
    // A Scryfall outage costs the pictures, as it does for the deck itself.
    printings: await fetchPrintings(parsed.cards).catch(() => new Map()),
    name:
      parsed.name ?? deckStem(viewing.kind === "deck" ? viewing.path : path),
    revision,
  };
}

/** The deck at `path` as `viewing` names it, or `null` while it is the deck now. */
export function useOther(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  viewing: Viewing,
): Other | null {
  const key =
    viewing.kind === "now"
      ? null
      : viewing.kind === "revision"
        ? `at:${viewing.commit}`
        : `vs:${viewing.path}`;
  const [other, setOther] = useState<{ key: string; other: Other } | null>(
    null,
  );
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is `viewing`
  useEffect(() => {
    if (viewing.kind === "now" || key === null) return;
    let live = true;
    setOther({ key, other: { kind: "loading" } });
    load(api, repo, path, viewing).then(
      (o) => live && setOther({ key, other: o }),
      (e) =>
        live &&
        setOther({
          key,
          other: {
            kind: "missing",
            message: e instanceof Error ? e.message : String(e),
          },
        }),
    );
    return () => {
      live = false;
    };
  }, [api, repo, path, key]);
  if (key === null) return null;
  return other?.key === key ? other.other : { kind: "loading" };
}
