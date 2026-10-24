import type {
  Conflict,
  ConflictHunk,
  FileConflict,
  FileSettle,
  MergeClash,
  Settle,
  Side,
} from "../deck.gen";
import "./clashes.css";

/**
 * A deck's clashes, one card (or one of the deck's fields) a row, each
 * with its two sides to pick from: `Sol Ring — quantity: main 2, budget 3`.
 * What a pick does is Rust's; this only shows the clashes and says which
 * side each row has picked. Every merge that can clash shows these: a
 * branch merge, a sync, a refused save.
 */
export function ClashPicks({
  clashes,
  picks,
  onPick,
  ours,
  theirs,
  group,
}: {
  clashes: readonly MergeClash[];
  /** The side picked for each clash, by its place in `clashes`. */
  picks: readonly (Side | undefined)[];
  onPick: (i: number, side: Side) => void;
  /** What `ours` and `theirs` are called here: `main`, `this browser`. */
  ours: string;
  theirs: string;
  /** Names the rows' radio groups, unique among those on the page. */
  group: string;
}) {
  return (
    <ul className="merge-card-clashes">
      {clashes.map((c, i) => (
        <li key={c.index} className="merge-card-clash">
          <span className="merge-card-clash-what">
            <strong>{c.subject}</strong> — {c.attribute}
          </span>
          <fieldset className="merge-card-clash-sides">
            <legend className="visually-hidden">
              {c.subject}, {c.attribute}
            </legend>
            {(["ours", "theirs"] as const).map((side) => (
              <label
                key={side}
                className={picks[i] === side ? "picked" : undefined}
              >
                <input
                  type="radio"
                  name={`${group}-${c.index}`}
                  checked={picks[i] === side}
                  onChange={() => onPick(i, side)}
                />
                <span className="merge-side-name">
                  {side === "ours" ? ours : theirs}
                </span>{" "}
                <span className="merge-side-value">
                  {side === "ours" ? c.ours : c.theirs}
                </span>
              </label>
            ))}
          </fieldset>
        </li>
      ))}
    </ul>
  );
}

/** `picks` with clash `i` set to `side`. */
export const picked = (
  picks: readonly (Side | undefined)[],
  i: number,
  side: Side,
) => picks.map((p, j) => (j === i ? side : p));

/** Every clash's pick, once each has one. */
export const allPicked = (
  picks: readonly (Side | undefined)[],
): Side[] | null =>
  picks.every((p) => p !== undefined) ? (picks as Side[]) : null;

/** Stretches of a file merged by lines that both changed, side by side. */
export function Hunks({
  hunks,
  ours,
  theirs,
}: {
  hunks: readonly ConflictHunk[];
  ours: string;
  theirs: string;
}) {
  return (
    <>
      {hunks.map((h, i) => (
        // biome-ignore lint/suspicious/noArrayIndexKey: a hunk has no identity but its place
        <div key={i} className="merge-clash">
          <div className="merge-clash-sides">
            <div>
              <h4>{ours} has</h4>
              <pre>{h.ours.join("\n") || "(nothing)"}</pre>
            </div>
            <div>
              <h4>{theirs} has</h4>
              <pre>{h.theirs.join("\n") || "(nothing)"}</pre>
            </div>
          </div>
        </div>
      ))}
    </>
  );
}

/** What the user has picked so far for one file of a conflict. */
export interface FileChoice {
  /** A side for each of a deck's clashes, by its place in `clashes`. */
  picks: (Side | undefined)[];
  /** The whole file one way. */
  whole?: Side;
}

/** A file's choice before anything is picked. */
export const unpicked = (f: FileConflict): FileChoice => ({
  picks: f.needs === "eachClash" ? f.clashes.map(() => undefined) : [],
});

/**
 * How `choice` settles `f`, once it has everything the file needs; null
 * until then, and always for a file that only settling everything one way
 * settles.
 */
export function fileSettleOf(
  f: FileConflict,
  choice: FileChoice,
): FileSettle | null {
  switch (f.needs) {
    case "eachClash": {
      const picks = allPicked(choice.picks);
      return picks && { needs: "eachClash", path: f.path, picks };
    }
    case "wholeFile":
      return choice.whole
        ? { needs: "wholeFile", path: f.path, side: choice.whole }
        : null;
    case "allOneWay":
      return null;
  }
}

/**
 * The settle of every file of `conflict` that can be settled on its own,
 * under the conflict's guard, once each has its picks; null until then,
 * or when no file can be.
 */
export function settleOf(
  conflict: Conflict,
  choiceOf: (f: FileConflict) => FileChoice,
): Settle | null {
  const own = conflict.files.filter((f) => f.needs !== "allOneWay");
  const files = own.map((f) => fileSettleOf(f, choiceOf(f)));
  if (files.length === 0 || files.some((s) => s === null)) return null;
  return { guard: conflict.guard, files: files as FileSettle[] };
}

/**
 * One file of a conflict and what settles it, as the conflict says: a side
 * for each of a deck's clashes, one side for a file whose lines clash or
 * whose merge would not read, or only settling everything one way.
 */
export function FileConflictPicks({
  file,
  choice,
  onChoice,
  ours,
  theirs,
  group,
}: {
  file: FileConflict;
  choice: FileChoice;
  onChoice: (next: FileChoice) => void;
  /** What `ours` and `theirs` are called here: `main`, `this browser`. */
  ours: string;
  theirs: string;
  /** Names the file's radio groups, unique among those on the page. */
  group: string;
}) {
  switch (file.needs) {
    case "eachClash":
      return (
        <ClashPicks
          clashes={file.clashes}
          picks={choice.picks}
          onPick={(i, side) =>
            onChoice({ ...choice, picks: picked(choice.picks, i, side) })
          }
          ours={ours}
          theirs={theirs}
          group={group}
        />
      );
    case "wholeFile":
      return (
        <>
          {file.why.kind === "lines" ? (
            <Hunks hunks={file.why.hunks} ours={ours} theirs={theirs} />
          ) : (
            <p className="refusal" role="alert">
              Merged, {file.path} would not be a file Progress Engine reads:{" "}
              {file.why.reason}. Pick a side and the whole file takes it.
            </p>
          )}
          <fieldset className="merge-card-clash-sides">
            <legend className="visually-hidden">{file.path}</legend>
            {(["ours", "theirs"] as const).map((side) => (
              <label
                key={side}
                className={choice.whole === side ? "picked" : undefined}
              >
                <input
                  type="radio"
                  name={`${group}-whole`}
                  checked={choice.whole === side}
                  onChange={() => onChoice({ ...choice, whole: side })}
                />{" "}
                {side === "ours" ? `Keep ${ours}'s` : `Take ${theirs}'s`}
              </label>
            ))}
          </fieldset>
        </>
      );
    case "allOneWay":
      return (
        <p>
          One side deleted or replaced it whole; only settling everything one
          way settles it.
        </p>
      );
  }
}
