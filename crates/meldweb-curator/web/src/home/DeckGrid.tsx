import { type DragEvent, useEffect, useRef, useState } from "react";
import type { MenuEntry } from "../card/CardView";
import type { DeckEntry } from "../github/decks";
import type { Printings } from "../scryfall";
import { DeckTile } from "./DeckTile";
import { type DeckMove, familyOrder, moveDeck, nudgeDeck } from "./variants";

type Drop = { path: string; side: "before" | "after" };

/**
 * The decks as tiles in `order`, each dragged onto another to move it there,
 * or moved with Alt+arrows or its menu. A deck carries its variants along.
 */
export function DeckGrid({
  decks,
  order,
  printings,
  menuFor,
  onMove,
}: {
  decks: readonly DeckEntry[];
  /** `meldweb.toml`'s deck order, by path. */
  order: readonly string[];
  printings: Printings;
  menuFor: (deck: DeckEntry) => MenuEntry[];
  /** Saves the list with `move` made, a move that moves something here. */
  onMove: (move: DeckMove) => void;
}) {
  const shown = familyOrder(decks, order);
  const [dragging, setDragging] = useState<string | null>(null);
  const [drop, setDrop] = useState<Drop | null>(null);
  // A tile moved by key is a new place in the DOM; focus follows it there.
  const [refocus, setRefocus] = useState<string | null>(null);
  const grid = useRef<HTMLUListElement>(null);
  useEffect(() => {
    if (!refocus) return;
    const tile = [
      ...(grid.current?.querySelectorAll<HTMLElement>("[data-deck]") ?? []),
    ].find((li) => li.dataset.deck === refocus);
    tile?.querySelector<HTMLElement>(".deck-tile")?.focus();
    setRefocus(null);
  });

  const nudge = (path: string, by: -1 | 1) => {
    if (!nudgeDeck(shown, path, by)) return;
    onMove({ kind: "nudge", path, by });
    setRefocus(path);
  };
  const sideOf = (e: DragEvent<HTMLElement>): Drop["side"] => {
    // Split along the diagonal, so it reads left/right across a row of tiles
    // and top/bottom down the one column a phone has.
    const r = e.currentTarget.getBoundingClientRect();
    const across = (e.clientX - r.left) / r.width;
    const down = (e.clientY - r.top) / r.height;
    return across + down > 1 ? "after" : "before";
  };
  const end = () => {
    setDragging(null);
    setDrop(null);
  };

  return (
    <ul className="deck-grid" ref={grid}>
      {shown.map((d) => {
        const target =
          drop?.path === d.path && dragging !== null
            ? moveDeck(shown, dragging, d.path, drop.side)
            : null;
        const menu = menuFor(d);
        return (
          <li
            key={d.path}
            data-deck={d.path}
            className={[
              dragging === d.path ? "dragging" : "",
              target ? `drop-${drop?.side}` : "",
            ].join(" ")}
            draggable
            onDragStart={(e) => {
              e.dataTransfer.effectAllowed = "move";
              setDragging(d.path);
            }}
            onDragEnd={end}
            onDragOver={(e) => {
              if (dragging === null) return;
              e.preventDefault();
              e.dataTransfer.dropEffect = "move";
              const side = sideOf(e);
              if (drop?.path !== d.path || drop.side !== side)
                setDrop({ path: d.path, side });
            }}
            onDrop={(e) => {
              e.preventDefault();
              if (dragging !== null) {
                const side = sideOf(e);
                if (moveDeck(shown, dragging, d.path, side))
                  onMove({ kind: "drop", from: dragging, to: d.path, side });
              }
              end();
            }}
            onKeyDown={(e) => {
              if (!e.altKey) return;
              const by =
                e.key === "ArrowLeft" || e.key === "ArrowUp"
                  ? -1
                  : e.key === "ArrowRight" || e.key === "ArrowDown"
                    ? 1
                    : 0;
              if (by === 0) return;
              e.preventDefault();
              nudge(d.path, by);
            }}
          >
            <DeckTile
              deck={d}
              printings={printings}
              menu={[
                ...menu,
                "separator",
                {
                  label: "Move earlier",
                  hotkey: "Alt+←",
                  disabled: nudgeDeck(shown, d.path, -1) === null,
                  run: () => nudge(d.path, -1),
                },
                {
                  label: "Move later",
                  hotkey: "Alt+→",
                  disabled: nudgeDeck(shown, d.path, 1) === null,
                  run: () => nudge(d.path, 1),
                },
              ]}
              parent={decks.find((p) => p.path === d.variantOf)?.name}
            />
          </li>
        );
      })}
    </ul>
  );
}
