import { useState } from "react";
import { allPicked, ClashPicks, Hunks, picked } from "../deck/ClashPicks";
import type { FileSettle, SyncConflict } from "../deck.gen";
import type { MergeSide } from "../repo/api";

/**
 * A file of a branch as the user reads it: `lantern.deck.toml` on a deck's
 * main or the repo's, `lantern.deck.toml on budget` on a deck's branch.
 */
export function conflictName(f: Pick<SyncConflict, "branch" | "path">) {
  const branch = /^decks\/[^/]+\/(.+)$/.exec(f.branch)?.[1] ?? f.branch;
  return branch === "main" ? f.path : `${f.path} on ${branch}`;
}

/** What one file still needs picked, and its settle once it has it all. */
type Choice = {
  picks: (MergeSide | undefined)[];
  /** The whole file one way: lines, or a merge that would not read. */
  whole?: MergeSide;
};

/** Whether `f` can be settled on its own, rather than only all one way. */
const settleable = (f: SyncConflict) =>
  f.clashes.length > 0 || f.hunks.length > 0 || f.unreadable !== undefined;

const needsWhole = (f: SyncConflict) =>
  f.hunks.length > 0 || f.unreadable !== undefined;

function settleOf(f: SyncConflict, c: Choice): FileSettle | null {
  const picks = allPicked(c.picks);
  if (!picks || (needsWhole(f) && !c.whole)) return null;
  return {
    branch: f.branch,
    path: f.path,
    ours: f.ours,
    theirs: f.theirs,
    picks,
    ...(c.whole ? { prefer: c.whole } : {}),
  };
}

/**
 * A sync's conflict, file by file and branch by branch: each deck's clashes
 * with a side to pick for each, another file's lines with a side for the
 * file, and two ways to settle everything one way. The picks go back keyed
 * by branch and path, guarded by the tips the clashes were shown from
 * (`FileSettle`), and the next sync makes the merge commit with them.
 */
export function SyncConflicts({
  files,
  host,
  disabled,
  onSettle,
  onResolve,
}: {
  files: readonly SyncConflict[];
  host: string;
  disabled: boolean;
  onSettle: (files: FileSettle[]) => void;
  onResolve: (side: MergeSide) => void;
}) {
  const key = (f: SyncConflict) => `${f.branch}\n${f.path}`;
  const [choices, setChoices] = useState<Record<string, Choice>>({});
  const choiceOf = (f: SyncConflict): Choice =>
    choices[key(f)] ?? { picks: f.clashes.map(() => undefined) };
  const update = (f: SyncConflict, next: Choice) =>
    setChoices((was) => ({ ...was, [key(f)]: next }));
  const settles = files.filter(settleable).map((f) => settleOf(f, choiceOf(f)));
  const ready =
    settles.length > 0 && settles.every((s): s is FileSettle => s !== null);
  const one = files.length === 1;
  return (
    <section
      className="merge-clashes sync-conflicts"
      aria-label="Where they clash"
    >
      {files.map((f) => {
        const c = choiceOf(f);
        const name = conflictName(f);
        return (
          <div key={key(f)} className="sync-conflict">
            <h4 className="merge-clash-path">{name}</h4>
            {f.clashes.length > 0 && (
              <ClashPicks
                clashes={f.clashes}
                picks={c.picks}
                onPick={(i, side) =>
                  update(f, { ...c, picks: picked(c.picks, i, side) })
                }
                ours="here"
                theirs={`on ${host}`}
                group={`sync-${key(f)}`}
              />
            )}
            <Hunks hunks={f.hunks} ours="This browser" theirs={host} />
            {f.unreadable !== undefined && (
              <p>
                Merged, it would not be a file Progress Engine reads:{" "}
                {f.unreadable}. Pick a side and the whole file takes it.
              </p>
            )}
            {needsWhole(f) && (
              <fieldset className="merge-card-clash-sides">
                <legend className="visually-hidden">{name}</legend>
                {(["ours", "theirs"] as const).map((side) => (
                  <label
                    key={side}
                    className={c.whole === side ? "picked" : undefined}
                  >
                    <input
                      type="radio"
                      name={`sync-whole-${key(f)}`}
                      checked={c.whole === side}
                      onChange={() => update(f, { ...c, whole: side })}
                    />{" "}
                    {side === "ours" ? "Keep this browser's" : `Take ${host}'s`}
                  </label>
                ))}
              </fieldset>
            )}
            {!settleable(f) && (
              <p>
                One side deleted or replaced it whole; only one of the choices
                below settles it.
              </p>
            )}
          </div>
        );
      })}
      <div className="rule-actions merge-pick">
        <button
          type="button"
          className="small primary"
          disabled={disabled || !ready}
          onClick={() =>
            onSettle(settles.filter((s): s is FileSettle => s !== null))
          }
        >
          Merge with these picks
        </button>
        <button
          type="button"
          className="small"
          disabled={disabled}
          onClick={() => onResolve("theirs")}
        >
          Take {host}'s side of {one ? "it" : "everything"}
        </button>
        <button
          type="button"
          className="small"
          disabled={disabled}
          onClick={() => onResolve("ours")}
        >
          Keep this browser's side of {one ? "it" : "everything"}
        </button>
      </div>
    </section>
  );
}
