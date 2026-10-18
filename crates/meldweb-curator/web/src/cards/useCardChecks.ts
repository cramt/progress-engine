import { useEffect, useState } from "react";
import type { GitHubApi, RepoRef } from "../github/api";
import { besideCard } from "../github/cards";
import { type Checked, checkCard, renderPng, whyReal } from "../mockery";
import { messageOf } from "../ui/Sheet";
import { realCandidates } from "./real";

/** Whether Magic already has the card, which only Scryfall can say. */
export type Verdict =
  | { kind: "checking" }
  | { kind: "custom" }
  | { kind: "real"; why: string }
  /** Scryfall could not be asked, so the card is not printed. */
  | { kind: "unchecked"; why: string };

/** Art read once a page, by repo path; null where the repo has none. */
const arts = new Map<string, Promise<Uint8Array | null>>();

export function artFor(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
): Promise<Uint8Array | null> {
  let art = arts.get(path);
  if (!art) {
    art = api.getBytes(repo, path).catch(() => null);
    arts.set(path, art);
  }
  return art;
}

/** The real cards' search, once per card text, so a sheet asks no twice. */
const verdicts = new Map<string, Promise<Verdict>>();

export function verdictFor(text: string, checked: Checked): Promise<Verdict> {
  if (checked.kind !== "card") return Promise.resolve({ kind: "custom" });
  let v = verdicts.get(text);
  if (!v) {
    v = realCandidates(checked).then(
      ({ named, sameCost }) => {
        const why = whyReal(text, named, sameCost);
        return why ? { kind: "real", why } : { kind: "custom" };
      },
      (e): Verdict => {
        verdicts.delete(text);
        return { kind: "unchecked", why: messageOf(e) };
      },
    );
    verdicts.set(text, v);
  }
  return v;
}

export interface CardView {
  checked: Checked;
  /** An object URL of the rendered card, or why it did not render. */
  preview:
    | { kind: "png"; url: string }
    | { kind: "failed"; why: string }
    | null;
  verdict: Verdict;
  /** The art the card names, missing from the repo. */
  missingArt: string | null;
}

/**
 * What the page shows of `text`: Insight's reading at once, the picture a
 * moment after typing stops, and Scryfall's verdict a moment after that.
 */
export function useCardChecks(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  text: string,
): CardView {
  const checked = checkCard(text);
  const [preview, setPreview] = useState<CardView["preview"]>(null);
  const [missingArt, setMissingArt] = useState<string | null>(null);
  const [verdict, setVerdict] = useState<Verdict>({ kind: "checking" });

  useEffect(() => {
    let live = true;
    let url: string | null = null;
    const t = setTimeout(async () => {
      const c = checkCard(text);
      if (c.kind !== "card") return;
      const artPath = c.art ? besideCard(path, c.art) : null;
      const art = artPath ? await artFor(api, repo, artPath) : null;
      if (!live) return;
      setMissingArt(artPath && !art ? artPath : null);
      try {
        // Art the repo lacks renders as a card without it, said beside it.
        const png = renderPng(
          artPath && !art ? text.replace(/^art\s*=.*$/m, "") : text,
          art,
          150,
        );
        url = URL.createObjectURL(
          new Blob([png as BlobPart], { type: "image/png" }),
        );
        setPreview({ kind: "png", url });
      } catch (e) {
        setPreview({ kind: "failed", why: messageOf(e) });
      }
    }, 250);
    return () => {
      live = false;
      clearTimeout(t);
      if (url) setTimeout(() => URL.revokeObjectURL(url as string), 1000);
    };
  }, [api, repo, path, text]);

  useEffect(() => {
    let live = true;
    setVerdict({ kind: "checking" });
    const t = setTimeout(() => {
      const c = checkCard(text);
      void verdictFor(text, c).then((v) => live && setVerdict(v));
    }, 800);
    return () => {
      live = false;
      clearTimeout(t);
    };
  }, [text]);

  return { checked, preview, verdict, missingArt };
}
