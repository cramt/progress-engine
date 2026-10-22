import type { ConflictHunk, MergeClash } from "../deck.gen";
import type { MergeSide } from "../repo/api";
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
  picks: readonly (MergeSide | undefined)[];
  onPick: (i: number, side: MergeSide) => void;
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
  picks: readonly (MergeSide | undefined)[],
  i: number,
  side: MergeSide,
) => picks.map((p, j) => (j === i ? side : p));

/** Every clash's pick, once each has one. */
export const allPicked = (
  picks: readonly (MergeSide | undefined)[],
): MergeSide[] | null =>
  picks.every((p) => p !== undefined) ? (picks as MergeSide[]) : null;

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
          <p className="merge-clash-path">{h.path}</p>
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
