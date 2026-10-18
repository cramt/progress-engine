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
  /** The art the card names, missing from the repo. */
  missingArt: string | null;
}

/**
 * What the page shows of `text`: Insight's reading at once, and the picture
 * a moment after typing stops.
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

  return { checked, preview, missingArt };
}
