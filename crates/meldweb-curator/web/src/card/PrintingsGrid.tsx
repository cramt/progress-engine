import { Link } from "@tanstack/react-router";
import { type KeyboardEvent, useEffect, useRef, useState } from "react";
import { printingId } from "../scryfall";
import { type RankedOption, usePreferredOrder } from "./preference";
import { byRelease, filterBySet, type PrintingOption } from "./prints";

type Order = "preference" | "newest" | "oldest";

/**
 * Archidekt's All printings: every printing of the card as a picture, filtered
 * by set name. By default they are in the order `meldweb.toml` prefers, each
 * labelled with the rules that moved it; newest or oldest first on request.
 * The deck's printing is outlined and labelled; clicking another picks it.
 * The arrow keys move between pictures and Enter picks, so a whole deck can be
 * walked from the keyboard.
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
  const [order, setOrder] = useState<Order>("preference");
  const preferred = usePreferredOrder(printings);
  const sorted: readonly RankedOption[] =
    order === "preference"
      ? preferred.ranked
      : byRelease(printings, order === "oldest").map((option) => ({
          option,
          matched: [],
        }));
  const kept = new Set(filterBySet(printings, filter));
  const shown = sorted.filter((r) => kept.has(r.option));

  const list = useRef<HTMLUListElement>(null);
  // A new card's printings put the cursor on its printing, or on the one the
  // order offers first, so Enter takes it.
  // biome-ignore lint/correctness/useExhaustiveDependencies: on a new list only
  useEffect(() => {
    const buttons = tiles(list.current);
    const at = buttons.find((b) => b.dataset.selected === "true") ?? buttons[0];
    at?.focus({ preventScroll: false });
  }, [printings]);

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
            value={order}
            onChange={(e) => setOrder(e.target.value as Order)}
          >
            <option value="preference">Your preference</option>
            <option value="newest">Release date, newest first</option>
            <option value="oldest">Release date, oldest first</option>
          </select>
        </label>
        <span className="printings-grid-count">
          {shown.length} of {printings.length} printings
        </span>
      </div>
      {order === "preference" && <RankedBy preferred={preferred} />}
      <ul
        className="printings-grid-list"
        ref={list}
        onKeyDown={(e) => moveCursor(e, list.current)}
      >
        {shown.map(({ option: p, matched }) => {
          const id = printingId(p);
          const selected = id === current;
          return (
            <li key={id}>
              <button
                type="button"
                className={selected ? "printing selected" : "printing"}
                aria-pressed={selected}
                data-selected={selected}
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
                {matched.length > 0 && (
                  <span className="printing-rules">
                    {matched.map((r) => (
                      <span
                        key={`${r.verb}:${r.query}`}
                        className={`printing-rule ${r.verb}`}
                      >
                        {r.verb === "prefer" ? "↑" : "↓"} {r.query}
                      </span>
                    ))}
                  </span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

/** What ranked the pictures, so an order that looks odd can be traced. */
function RankedBy({
  preferred,
}: {
  preferred: ReturnType<typeof usePreferredOrder>;
}) {
  const edit = <Link to="/settings">Edit the rules</Link>;
  if (preferred.kind === "refused")
    return (
      <p className="printings-grid-note refusal" role="alert">
        {preferred.message}. Newest first instead. {edit}
      </p>
    );
  return (
    <p className="printings-grid-note">
      {preferred.declared
        ? "Ranked by the rules in meldweb.toml."
        : "Ranked by the default rules."}{" "}
      {edit}
    </p>
  );
}

function tiles(list: HTMLUListElement | null): HTMLButtonElement[] {
  return list ? [...list.querySelectorAll<HTMLButtonElement>("button")] : [];
}

/**
 * Arrow keys between pictures: left and right by one, up and down by a row,
 * the row read off the layout since the grid's column count follows the
 * window.
 */
function moveCursor(e: KeyboardEvent, list: HTMLUListElement | null) {
  const buttons = tiles(list);
  const from = buttons.indexOf(document.activeElement as HTMLButtonElement);
  if (from < 0 || e.shiftKey || e.altKey || e.ctrlKey || e.metaKey) return;
  const top = buttons[0]?.offsetTop;
  const columns = Math.max(
    1,
    buttons.findIndex((b) => b.offsetTop !== top) < 0
      ? buttons.length
      : buttons.findIndex((b) => b.offsetTop !== top),
  );
  const step: Record<string, number> = {
    ArrowLeft: -1,
    ArrowRight: 1,
    ArrowUp: -columns,
    ArrowDown: columns,
  };
  const by = step[e.key];
  if (by === undefined) return;
  e.preventDefault();
  // The modal's own arrows step through the deck; here they move the cursor.
  e.stopPropagation();
  const to = Math.min(buttons.length - 1, Math.max(0, from + by));
  buttons[to]?.focus();
}
