import { useState } from "react";
import {
  type FileChoice,
  FileConflictPicks,
  settleOf,
  unpicked,
} from "../deck/ClashPicks";
import type { FileConflict, Settle, Side, SyncConflict } from "../deck.gen";

/**
 * A file of a branch as the user reads it: `lantern.deck.toml` on a deck's
 * main or the repo's, `lantern.deck.toml on budget` on a deck's branch.
 */
export function conflictName(branch: string, path: string) {
  const own = /^decks\/[^/]+\/(.+)$/.exec(branch)?.[1] ?? branch;
  return own === "main" ? path : `${path} on ${own}`;
}

/** Every file a sync stopped on, by its name. */
export const conflictNames = (conflicts: readonly SyncConflict[]) =>
  conflicts.flatMap((c) =>
    c.conflict.files.map((f) => conflictName(c.branch, f.path)),
  );

/**
 * A sync's conflict, branch by branch and file by file, each file with what
 * settles it (`FileConflictPicks`), and two ways to settle everything one
 * way. Each branch's picks go back under its conflict's guard, so Rust
 * settles that merge with them and no later one, and the next sync makes
 * the merge commit with them.
 */
export function SyncConflicts({
  conflicts,
  host,
  disabled,
  onSettle,
  onResolve,
}: {
  conflicts: readonly SyncConflict[];
  host: string;
  disabled: boolean;
  onSettle: (settles: Settle[]) => void;
  onResolve: (side: Side) => void;
}) {
  const key = (c: SyncConflict, f: FileConflict) => `${c.branch}\n${f.path}`;
  const [choices, setChoices] = useState<Record<string, FileChoice>>({});
  const choiceIn = (c: SyncConflict) => (f: FileConflict) =>
    choices[key(c, f)] ?? unpicked(f);
  const settleable = conflicts.filter((c) =>
    c.conflict.files.some((f) => f.needs !== "allOneWay"),
  );
  const settles = settleable.map((c) => settleOf(c.conflict, choiceIn(c)));
  const ready =
    settles.length > 0 && settles.every((s): s is Settle => s !== null);
  const one =
    conflicts.length === 1 && conflicts[0]?.conflict.files.length === 1;
  return (
    <section
      className="merge-clashes sync-conflicts"
      aria-label="Where they clash"
    >
      {conflicts.flatMap((c) =>
        c.conflict.files.map((f) => (
          <div key={key(c, f)} className="sync-conflict">
            <h4 className="merge-clash-path">
              {conflictName(c.branch, f.path)}
            </h4>
            <FileConflictPicks
              file={f}
              choice={choiceIn(c)(f)}
              onChoice={(next) =>
                setChoices((was) => ({ ...was, [key(c, f)]: next }))
              }
              ours="this browser"
              theirs={host}
              group={`sync-${key(c, f)}`}
            />
          </div>
        )),
      )}
      <div className="rule-actions merge-pick">
        <button
          type="button"
          className="small primary"
          disabled={disabled || !ready}
          onClick={() =>
            onSettle(settles.filter((s): s is Settle => s !== null))
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
