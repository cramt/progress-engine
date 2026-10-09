import { useState } from "react";
import { type CopyState, useCopyState } from "./index";
import "./copy.css";

const MB = 1_000_000;

function megabytes(received: number, total: number): string {
  const got = Math.round(received / MB);
  return total > 0 ? `${got} of ${Math.round(total / MB)} MB` : `${got} MB`;
}

/** What the corner says about the copy, or `null` while there is nothing to say. */
function line(state: CopyState): { text: string; progress?: number } | null {
  switch (state.kind) {
    case "downloading":
      return {
        text: `Copying Scryfall's cards, ${megabytes(state.received, state.total)}. Until then, cards come from Scryfall's API.`,
        ...(state.total > 0 ? { progress: state.received / state.total } : {}),
      };
    case "saving":
      return { text: "Saving the copy of Scryfall's cards" };
    case "ready":
      return state.refresh
        ? {
            text: `Updating the copy of Scryfall's cards, ${megabytes(state.refresh.received, state.refresh.total)}`,
            ...(state.refresh.total > 0
              ? { progress: state.refresh.received / state.refresh.total }
              : {}),
          }
        : null;
    case "failed":
      return {
        text: `Cards come from Scryfall's API: copying them failed (${state.message}).`,
      };
    case "opening":
    case "unavailable":
      return null;
  }
}

/**
 * The page's copy of Scryfall (ADR-0030) while it is being made or has
 * failed: a first visit downloads about 85 MB of Scryfall's bulk files, and
 * says so.
 * Once the copy answers, the corner is empty.
 */
export function CopyStatus() {
  const state = useCopyState();
  const [dismissed, setDismissed] = useState<CopyState["kind"] | null>(null);
  const shown = line(state);
  if (!shown || dismissed === state.kind) return null;
  return (
    <div className="copy-status" role="status">
      <span>{shown.text}</span>
      {shown.progress !== undefined && (
        <progress value={shown.progress} max={1} />
      )}
      <button
        type="button"
        className="copy-status-close"
        aria-label="Hide"
        onClick={() => setDismissed(state.kind)}
      >
        ×
      </button>
    </div>
  );
}
