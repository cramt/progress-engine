import { useEffect, useState } from "react";
import type { GitHubApi, RepoRef } from "../github/api";
import { besideCard } from "../github/cards";
import { type Checked, checkCard, renderPng } from "../mockery";
import { messageOf } from "../ui/Sheet";

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

export interface CardView {
  checked: Checked;
  /** An object URL of the rendered card, or why it did not render. */
  preview:
    | { kind: "png"; url: string }
    | { kind: "failed"; why: string }
    | null;
  /** The arts the card names, missing from the repo. */
  missingArt: readonly string[];
}

/**
 * What the page shows of `text`: Insight's reading at once, and the picture
 * of card image `face` (a double-faced card's back is 1, a meld pair's
 * counterpart 2) a moment after typing
 * stops.
 */
export function useCardChecks(
  api: GitHubApi,
  repo: RepoRef,
  path: string,
  text: string,
  face: number,
): CardView {
  const checked = checkCard(text);
  const [preview, setPreview] = useState<CardView["preview"]>(null);
  const [missingArt, setMissingArt] = useState<readonly string[]>([]);

  useEffect(() => {
    let live = true;
    let url: string | null = null;
    const t = setTimeout(async () => {
      const c = checkCard(text);
      if (c.kind !== "card") return;
      const art = new Map<string, Uint8Array>();
      const missing: string[] = [];
      for (const a of c.arts) {
        const bytes = await artFor(api, repo, besideCard(path, a));
        if (bytes) art.set(a, bytes);
        else missing.push(besideCard(path, a));
      }
      if (!live) return;
      setMissingArt(missing);
      try {
        // Art the repo lacks renders as a card without it, said beside it.
        const png = renderPng(
          text,
          art,
          Math.min(face, c.images.length - 1),
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
  }, [api, repo, path, text, face]);

  return { checked, preview, missingArt };
}
