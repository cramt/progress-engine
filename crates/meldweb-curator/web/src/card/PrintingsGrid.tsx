import { useState } from "react";
import { printingId } from "../scryfall";
import { byRelease, filterBySet, type PrintingOption } from "./prints";

/**
 * Archidekt's All printings: every printing of the card as a picture, newest
 * first, filtered by set name. The deck's printing is outlined and labelled;
 * clicking another picks it.
 */
export function PrintingsGrid({
  printings,
  current,
  onPick,
}: {
  printings: readonly PrintingOption[];
  /** The deck's printing as `set/num`, or null when the card is named. */
  current: string | null;
  onPick: (printing: PrintingOption) => void;
}) {
  const [filter, setFilter] = useState("");
  const [oldest, setOldest] = useState(false);
  const shown = byRelease(filterBySet(printings, filter), oldest);
  return (
    <section className="printings-grid" aria-label="All printings">
      <div className="printings-grid-bar">
        <input
          type="search"
          placeholder="Filter by set name"
          aria-label="Filter by set name"
          value={filter}
          onChange={(e) => setFilter(e.target.value)}
        />
        <label>
          Order by{" "}
          <select
            value={oldest ? "oldest" : "newest"}
            onChange={(e) => setOldest(e.target.value === "oldest")}
          >
            <option value="newest">Release date, newest first</option>
            <option value="oldest">Release date, oldest first</option>
          </select>
        </label>
        <span className="printings-grid-count">
          {shown.length} of {printings.length} printings
        </span>
      </div>
      <ul className="printings-grid-list">
        {shown.map((p) => {
          const id = printingId(p);
          const selected = id === current;
          return (
            <li key={id}>
              <button
                type="button"
                className={selected ? "printing selected" : "printing"}
                aria-pressed={selected}
                title={`${p.setName} (${p.set.toUpperCase()}) #${p.num}`}
                onClick={() => onPick(p)}
              >
                {p.image ? (
                  <img
                    crossOrigin="anonymous"
                    src={p.image}
                    alt={p.name}
                    loading="lazy"
                  />
                ) : (
                  <span className="card-missing">{p.name}</span>
                )}
                {selected && (
                  <span className="printing-selected-label">
                    Selected printing
                  </span>
                )}
                <span className="printing-set">{p.setName}</span>
                <span className="printing-meta">
                  {p.set.toUpperCase()} #{p.num} · {p.released}
                </span>
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
