import { useEffect, useMemo, useState } from "react";
import { compareDecks } from "../deck";
import type { ConflictHunk } from "../deck.gen";
import {
  type MergeDone,
  type MergePlan,
  mergeAsPlanned,
  planSummary,
} from "../github/branches";
import type { MergeAsk, Repo } from "../repo/api";
import { messageOf, Sheet, useBusy } from "../ui/Sheet";
import { GROUP_TITLES, grouped, takeable } from "./versions";

type Planning =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "ready"; plan: MergePlan };

/**
 * Merges one of the deck's branches into another (ADR-0041): a branch into
 * main, or main into a branch to bring it up to date. It shows what the
 * merge will change before anything moves; where both changed the same
 * lines, it shows them and asks whose to keep.
 */
export function MergeDialog({
  repo,
  stem,
  from,
  into,
  prepare,
  onClose,
  onMerged,
}: {
  repo: Repo;
  stem: string;
  from: string;
  into: string;
  /** Lands the open branch's pending saves; resolves to why it could not. */
  prepare: () => Promise<string | null>;
  onClose: () => void;
  onMerged: (done: Extract<MergeDone, { kind: "merged" }>) => void;
}) {
  const [prefer, setPrefer] = useState<MergeAsk["prefer"]>();
  const [planning, setPlanning] = useState<Planning>({ kind: "loading" });
  const [round, setRound] = useState(0);
  const { busy, refusal, run } = useBusy();
  const ask: MergeAsk = {
    from,
    into,
    ...(prefer === undefined ? {} : { prefer }),
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: `round` plans again after a refused merge
  useEffect(() => {
    let live = true;
    setPlanning({ kind: "loading" });
    (async () => {
      const unsaved = await prepare();
      if (unsaved !== null)
        return { kind: "failed" as const, message: unsaved };
      return {
        kind: "ready" as const,
        plan: await repo.planMerge(stem, {
          from,
          into,
          ...(prefer ? { prefer } : {}),
        }),
      };
    })().then(
      (p) => live && setPlanning(p),
      (e) => live && setPlanning({ kind: "failed", message: messageOf(e) }),
    );
    return () => {
      live = false;
    };
  }, [repo, stem, from, into, prefer, round]);

  const plan = planning.kind === "ready" ? planning.plan : null;
  const mergeable = plan?.kind === "merge" || plan?.kind === "fastForward";
  return (
    <Sheet label={`Merge ${from} into ${into}`} onClose={onClose} wide>
      <form
        className="merge-dialog"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!plan) return;
          const ok = await run(async () => {
            const done = await mergeAsPlanned(repo, stem, ask, plan);
            if (done.kind === "merged") {
              onMerged(done);
              return null;
            }
            setRound((r) => r + 1);
            return done.kind === "upToDate"
              ? `${into} already has everything on ${from}.`
              : done.message;
          });
          if (ok) onClose();
        }}
      >
        <h2>
          Merge <code>{from}</code> into <code>{into}</code>
        </h2>
        {planning.kind === "loading" && (
          <p className="hint">Working out the merge…</p>
        )}
        {planning.kind === "failed" && (
          <p className="refusal" role="alert">
            {planning.message}
          </p>
        )}
        {plan && <p className="hint">{planSummary(plan, ask)}</p>}
        {prefer && (
          <p className="notice">
            Where they clash, {prefer === "ours" ? into : from}'s lines are
            kept.{" "}
            <button
              type="button"
              className="small"
              onClick={() => setPrefer(undefined)}
            >
              Change
            </button>
          </p>
        )}
        {plan?.kind === "conflict" && (
          <Clashes
            hunks={plan.hunks}
            unreadable={plan.unreadable}
            from={from}
            into={into}
            onPick={setPrefer}
          />
        )}
        {(plan?.kind === "merge" || plan?.kind === "fastForward") && (
          <WhatChanges into={into} before={plan.before} after={plan.after} />
        )}
        {plan?.kind === "merge" && (
          <p className="merge-message">
            Commit message: <code>{plan.message.split("\n", 1)[0]}</code>
          </p>
        )}
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            {mergeable ? "Cancel" : "Close"}
          </button>
          {mergeable && (
            <button type="submit" className="primary" disabled={busy}>
              {busy ? "Merging…" : `Merge into ${into}`}
            </button>
          )}
        </div>
      </form>
    </Sheet>
  );
}

/** What the branch merged into will change, as the deck's own diff says it. */
function WhatChanges({
  into,
  before,
  after,
}: {
  into: string;
  before: string | undefined;
  after: string | undefined;
}) {
  const compared = useMemo(
    () =>
      before !== undefined && after !== undefined
        ? compareDecks(before, after)
        : null,
    [before, after],
  );
  if (!compared) return null;
  if (compared.kind === "refused") {
    return (
      <p className="refusal" role="alert">
        {compared.message}
      </p>
    );
  }
  const changes = takeable(compared.changes, { kind: "revision", commit: "" });
  if (changes.length === 0) {
    return (
      <p className="version-same">
        {into}'s list stays the same, card for card.
      </p>
    );
  }
  return (
    <section className="changes" aria-label={`What ${into} will change`}>
      <h3>
        What {into} will change: {changes.length} difference
        {changes.length === 1 ? "" : "s"}
      </h3>
      <div className="changes-groups">
        {grouped(changes).map(({ group, changes: rows }) => (
          <div key={group} className={`changes-group changes-${group}`}>
            <span className="changes-group-title">
              {GROUP_TITLES[group]}
              <span className="badge">{rows.length}</span>
            </span>
            <ul>
              {rows.map(({ at, change }) => (
                <li key={at} className="change">
                  <span className="change-text">{change.text}</span>
                </li>
              ))}
            </ul>
          </div>
        ))}
      </div>
    </section>
  );
}

/** The stretches both changed, side by side, and the choice that settles them. */
function Clashes({
  hunks,
  unreadable,
  from,
  into,
  onPick,
}: {
  hunks: readonly ConflictHunk[];
  unreadable: readonly { path: string; reason: string }[];
  from: string;
  into: string;
  onPick: (side: "ours" | "theirs") => void;
}) {
  return (
    <section className="merge-clashes" aria-label="Where they clash">
      {hunks.map((h, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: a hunk has no identity but its place
        <div key={i} className="merge-clash">
          <p className="merge-clash-path">{h.path}</p>
          <div className="merge-clash-sides">
            <div>
              <h4>{into} has</h4>
              <pre>{h.ours.join("\n") || "(nothing)"}</pre>
            </div>
            <div>
              <h4>{from} has</h4>
              <pre>{h.theirs.join("\n") || "(nothing)"}</pre>
            </div>
          </div>
        </div>
      ))}
      {unreadable.map((u) => (
        <p key={u.path} className="refusal" role="alert">
          Merged line by line, {u.path} would not be a deck Progress Engine
          reads: {u.reason}
        </p>
      ))}
      <div className="dialog-buttons merge-pick">
        <button type="button" onClick={() => onPick("ours")}>
          Keep {into}'s lines
        </button>
        <button type="button" onClick={() => onPick("theirs")}>
          Take {from}'s lines
        </button>
      </div>
    </section>
  );
}
