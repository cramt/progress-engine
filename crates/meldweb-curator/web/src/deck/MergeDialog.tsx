import { type ReactNode, useEffect, useMemo, useState } from "react";
import { compareDecks } from "../deck";
import type { Conflict, FileConflict } from "../deck.gen";
import {
  type MergeDone,
  type MergePlan,
  planSummary,
} from "../github/branches";
import type { MergeAsk, Repo, Settle, Side } from "../repo/api";
import { messageOf, Sheet, useBusy } from "../ui/Sheet";
import {
  type FileChoice,
  FileConflictPicks,
  settleOf,
  unpicked,
} from "./ClashPicks";
import { GROUP_TITLES, grouped, takeable } from "./versions";

type Planning =
  | { kind: "loading" }
  | { kind: "failed"; message: string }
  | { kind: "ready"; plan: MergePlan };

/** The two tips a plan was worked out from, which its merge is guarded by. */
export interface MergeTips {
  intoTip: string;
  fromTip: string;
}

/**
 * Where a merge comes from: planning it, and making it at the tips the plan
 * showed. Rust refuses a merge whose tips or picks are not the plan's.
 */
export interface MergeSource {
  plan(settle?: Settle): Promise<MergePlan>;
  merge(tips: MergeTips, settle?: Settle): Promise<MergeDone>;
}

/**
 * Merges one of the deck's branches into another (ADR-0041): a branch into
 * main, or main into a branch to bring it up to date; or, given `source`,
 * another player's deck the fork follows into its main. It shows what the
 * merge will change before anything moves. A deck merges card by card, so
 * where both changed the same thing about a card (or about the deck) it
 * shows that, `Sol Ring — quantity: main 2, budget 3`, and asks whose to
 * keep, one clash at a time; another file both changed in the same lines
 * asks whose lines to keep.
 */
export function MergeDialog({
  repo,
  stem,
  from,
  into,
  prepare,
  source,
  children,
  onClose,
  onMerged,
}: {
  repo: Repo;
  stem: string;
  /** What is merged, as the dialog names it: a branch, or `alice's Lantern`. */
  from: string;
  into: string;
  /** Lands the open branch's pending saves; resolves to why it could not. */
  prepare: () => Promise<string | null>;
  /** Defaults to the deck's branch `from` merged into its branch `into`. */
  source?: MergeSource;
  /** Shown above what the merge changes. */
  children?: ReactNode;
  onClose: () => void;
  onMerged: (done: Extract<MergeDone, { kind: "merged" }>) => void;
}) {
  const [settle, setSettle] = useState<Settle | undefined>();
  const [planning, setPlanning] = useState<Planning>({ kind: "loading" });
  const [round, setRound] = useState(0);
  const { busy, refusal, run } = useBusy();
  const ask: MergeAsk = { from, into, ...(settle ? { settle } : {}) };
  const merging: MergeSource = source ?? {
    plan: (s) =>
      repo.planMerge(stem, { from, into, ...(s ? { settle: s } : {}) }),
    merge: (tips, s) =>
      repo.merge(stem, { from, into, ...tips, ...(s ? { settle: s } : {}) }),
  };

  // biome-ignore lint/correctness/useExhaustiveDependencies: `round` plans again after a refused merge, and `merging` is made anew each render from what is listed
  useEffect(() => {
    let live = true;
    setPlanning({ kind: "loading" });
    (async () => {
      const unsaved = await prepare();
      if (unsaved !== null)
        return { kind: "failed" as const, message: unsaved };
      return { kind: "ready" as const, plan: await merging.plan(settle) };
    })().then(
      (p) => live && setPlanning(p),
      (e) => live && setPlanning({ kind: "failed", message: messageOf(e) }),
    );
    return () => {
      live = false;
    };
  }, [repo, stem, from, into, settle, round]);

  const plan = planning.kind === "ready" ? planning.plan : null;
  const mergeable = plan?.kind === "merge" || plan?.kind === "fastForward";
  return (
    <Sheet label={`Merge ${from} into ${into}`} onClose={onClose} wide>
      <form
        className="merge-dialog"
        onSubmit={async (e) => {
          e.preventDefault();
          if (plan?.kind !== "merge" && plan?.kind !== "fastForward") return;
          const ok = await run(async () => {
            const done = await merging.merge(
              { intoTip: plan.into, fromTip: plan.from },
              settle,
            );
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
        {children}
        {settle && plan?.kind !== "conflict" && plan?.kind !== "stopped" && (
          <p className="notice">
            {settledSummary(settle, into, from)}{" "}
            <button
              type="button"
              className="small"
              onClick={() => setSettle(undefined)}
            >
              Change
            </button>
          </p>
        )}
        {plan?.kind === "stopped" && (
          <p className="notice">
            <button
              type="button"
              className="small"
              onClick={() => setSettle(undefined)}
            >
              Look at it again
            </button>
          </p>
        )}
        {plan?.kind === "conflict" && (
          <Clashes
            conflict={plan.conflict}
            from={from}
            into={into}
            onSettle={setSettle}
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
export function WhatChanges({
  into,
  before,
  after,
  title,
}: {
  into: string;
  before: string | undefined;
  after: string | undefined;
  /** The heading, `What main will change` when absent. */
  title?: string;
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
        {title
          ? `${title}: nothing, card for card.`
          : `${into}'s list stays the same, card for card.`}
      </p>
    );
  }
  return (
    <section
      className="changes"
      aria-label={title ?? `What ${into} will change`}
    >
      <h3>
        {title ?? `What ${into} will change`}: {changes.length} difference
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

/** What the user picked, said back above the merge it makes. */
function settledSummary(settle: Settle, into: string, from: string): string {
  const name = (side: Side) => (side === "ours" ? into : from);
  const files = settle.files ?? [];
  const picks = files.flatMap((f) => (f.needs === "eachClash" ? f.picks : []));
  const wholes = files.flatMap((f) =>
    f.needs === "wholeFile" ? [`${f.path} as ${name(f.side)} has it`] : [],
  );
  if (files.length === 0 && settle.prefer)
    return `Where they clash, ${name(settle.prefer)}'s side is kept.`;
  const theirs = picks.filter((p) => p === "theirs").length;
  const said =
    picks.length === 0
      ? []
      : [
          `${picks.length} clash${picks.length === 1 ? "" : "es"} settled: ${theirs} from ${from}, ${picks.length - theirs} kept as ${into} has ${picks.length - theirs === 1 ? "it" : "them"}`,
        ];
  return `${[...said, ...wholes].join("; ")}.`;
}

/**
 * Everything both changed differently, file by file, and the choices that
 * settle it: what each file needs (`FileConflictPicks`), or one side for
 * everything. The picks go back under the conflict's guard, so they settle
 * this merge and no later one.
 */
function Clashes({
  conflict,
  from,
  into,
  onSettle,
}: {
  conflict: Conflict;
  from: string;
  into: string;
  onSettle: (settle: Settle) => void;
}) {
  const [choices, setChoices] = useState<Record<string, FileChoice>>({});
  const choiceOf = (f: FileConflict) => choices[f.path] ?? unpicked(f);
  const all = (side: Side) => onSettle({ guard: conflict.guard, prefer: side });
  const picks = settleOf(conflict, choiceOf);
  const own = conflict.files.every((f) => f.needs !== "allOneWay");
  const clashes = conflict.files.some((f) => f.needs === "eachClash");
  return (
    <section className="merge-clashes" aria-label="Where they clash">
      {conflict.files.map((f) => (
        <div key={f.path} className="merge-clash-file">
          {conflict.files.length > 1 && (
            <p className="merge-clash-path">{f.path}</p>
          )}
          <FileConflictPicks
            file={f}
            choice={choiceOf(f)}
            onChoice={(next) =>
              setChoices((was) => ({ ...was, [f.path]: next }))
            }
            ours={into}
            theirs={from}
            group={`clash-${f.path}`}
          />
        </div>
      ))}
      <div className="dialog-buttons merge-pick">
        <button type="button" onClick={() => all("ours")}>
          Keep {into}'s side of {clashes ? "every clash" : "them"}
        </button>
        <button type="button" onClick={() => all("theirs")}>
          Take {from}'s side of {clashes ? "every clash" : "them"}
        </button>
        {own && (
          <button
            type="button"
            className="primary"
            disabled={picks === null}
            onClick={() => picks && onSettle(picks)}
          >
            Use these picks
          </button>
        )}
      </div>
    </section>
  );
}
