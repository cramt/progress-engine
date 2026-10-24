import { useCallback, useEffect, useState } from "react";
import type { Following } from "../deck.gen";
import { refreshTheirs } from "../remote/follow";
import type { Repo } from "../repo/api";
import { messageOf, Sheet } from "../ui/Sheet";
import { Spinner } from "../ui/Spinner";
import { type MergeSource, WhatChanges } from "./MergeDialog";

export type UpstreamState =
  | { kind: "none" }
  | {
      kind: "following";
      following: Following;
      /** Their deck is being fetched. */
      fetching: boolean;
      /** Why the last fetch failed; what was fetched before still stands. */
      failed: string | null;
    };

/** `alice's Lantern`, as the page names the deck a fork follows. */
export function theirLabel(f: Following): string {
  const name = f.theirs?.name ?? f.upstream.deck;
  const branch = f.upstream.branch === "main" ? "" : ` ${f.upstream.branch}`;
  return `${f.upstream.owner}'s ${name}${branch}`;
}

/**
 * The deck another player's that this deck follows (ADR-0041), fetched when
 * the deck opens and when the page comes back into view, no more often than
 * a sync fetches, with no login: their repo is public.
 */
export function useUpstream(repo: Repo, stem: string) {
  const [state, setState] = useState<UpstreamState>({ kind: "none" });

  const refresh = useCallback(
    async (force = false) => {
      const was = await repo.following(stem);
      if (!was) return setState({ kind: "none" });
      setState((s) => ({
        kind: "following",
        following: was,
        fetching: true,
        failed: s.kind === "following" ? s.failed : null,
      }));
      let failed: string | null = null;
      try {
        await refreshTheirs(
          repo,
          {
            url: was.upstream.url,
            deck: was.upstream.deck,
            branch: was.upstream.branch,
          },
          { force },
        );
      } catch (e) {
        failed = messageOf(e);
      }
      const now = (await repo.following(stem)) ?? was;
      setState({ kind: "following", following: now, fetching: false, failed });
    },
    [repo, stem],
  );

  useEffect(() => {
    void refresh();
    const again = () => {
      if (document.visibilityState === "visible") void refresh();
    };
    document.addEventListener("visibilitychange", again);
    window.addEventListener("focus", again);
    return () => {
      document.removeEventListener("visibilitychange", again);
      window.removeEventListener("focus", again);
    };
  }, [refresh]);

  /** Their deck's changes, merged into the deck's main as planned. */
  const source: MergeSource = {
    plan: (settle) => repo.planTake(stem, settle),
    async merge(plan, settle) {
      if (plan.kind === "upToDate") return { kind: "upToDate" };
      if (plan.kind !== "fastForward" && plan.kind !== "merge")
        return { kind: "stopped", message: "Pick whose side to keep first." };
      return repo.takeUpstream(stem, {
        intoTip: plan.into,
        fromTip: plan.from,
        ...(settle ? { settle } : {}),
      });
    },
  };

  return { state, refresh, source };
}

/**
 * The bar on a fork of another player's deck: what is new on theirs, a
 * preview and merge of it, what this deck changed since, and unfollowing.
 */
export function UpstreamBar({
  state,
  onTake,
  onSince,
  onUnfollow,
  onRefresh,
}: {
  state: Extract<UpstreamState, { kind: "following" }>;
  onTake: () => void;
  onSince: () => void;
  onUnfollow: () => void;
  onRefresh: () => void;
}) {
  const f = state.following;
  const theirs = f.theirs;
  const label = theirLabel(f);
  return (
    <div className="upstream-bar notice" role="status">
      <span>
        Fork of <strong title={f.upstream.url}>{label}</strong>
        {state.fetching && <Spinner />}
      </span>
      {theirs ? (
        theirs.new > 0 ? (
          <button type="button" className="primary small" onClick={onTake}>
            {theirs.new} new on {label}: preview and merge
          </button>
        ) : (
          <span className="muted">Up to date with {label}.</span>
        )
      ) : (
        <span className="muted">Not fetched yet.</span>
      )}
      {theirs?.base && (
        <button type="button" className="small" onClick={onSince}>
          What you changed since
        </button>
      )}
      <button
        type="button"
        className="small"
        onClick={onRefresh}
        disabled={state.fetching}
      >
        Fetch
      </button>
      <button
        type="button"
        className="small"
        onClick={onUnfollow}
        title="Keep the deck and its history, and stop taking their changes"
      >
        Unfollow
      </button>
      {state.failed && <span className="refusal-inline">{state.failed}</span>}
    </div>
  );
}

/** What they changed since the two last agreed: from the merge base to their tip. */
export function TheirChanges({ following }: { following: Following }) {
  const t = following.theirs;
  if (!t?.baseText || !t.text) return null;
  return (
    <WhatChanges
      into="main"
      before={t.baseText}
      after={t.text}
      title={`What ${following.upstream.owner} changed`}
    />
  );
}

/** What this deck changed since it last agreed with theirs. */
export function SinceForked({
  following,
  onClose,
}: {
  following: Following;
  onClose: () => void;
}) {
  const t = following.theirs;
  return (
    <Sheet label="What you changed since forking" onClose={onClose} wide>
      <div className="merge-dialog">
        <h2>What you changed since {theirLabel(following)}</h2>
        <p className="hint">
          From where the two last agreed: the fork, or the last time you took
          their changes.
        </p>
        <WhatChanges
          into="main"
          before={t?.baseText}
          after={t?.mineText}
          title="What you changed"
        />
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Close
          </button>
        </div>
      </div>
    </Sheet>
  );
}
