import { type DragEvent, useEffect, useRef, useState } from "react";
import { CardView, type CardViewProps } from "../card/CardView";
import type { Category, Card as DeckCard } from "../deck";
import { cardName, type Printings, printingKey } from "../scryfall";
import {
  type Group,
  groupByCategory,
  packColumns,
  UNCATEGORIZED,
} from "./layout";

// Card geometry, in px. A card is as wide as fills the page with whole
// columns, between these two, or up to MAX_ALONE when a phone fits only one;
// the peek is the name bar a stacked card leaves showing, and follows from
// the width.
const MIN_W = 200;
const MAX_W = 250;
const MAX_ALONE = 340;
const HEADER = 40;
const GAP = 20;

interface Geometry {
  columns: number;
  width: number;
  height: number;
  peek: number;
}

function geometry(available: number): Geometry {
  const columns = Math.max(1, Math.floor((available + GAP) / (MIN_W + GAP)));
  const fits = Math.floor((available - (columns - 1) * GAP) / columns);
  const width = Math.max(
    MIN_W,
    Math.min(columns > 1 ? MAX_W : MAX_ALONE, fits),
  );
  const height = Math.round((width * 88) / 63);
  return { columns, width, height, peek: Math.round(height * 0.112) };
}

const stackHeight = (at: Geometry) => (g: Group) =>
  HEADER + (g.cards.length - 1) * at.peek + at.height + GAP;

/** Where a card was dropped: a category, or a place on the strip that may not be one yet. */
export type DropTarget =
  | { kind: "category"; name: string }
  | { kind: "new"; name: string }
  | { kind: "type"; type: "maybeboard" | "sideboard" };

export type OnDrop = (
  card: DeckCard,
  from: string | null,
  to: DropTarget,
  secondary: boolean,
) => void;

/** The card being dragged, and the group it was dragged out of. */
interface Dragging {
  card: DeckCard;
  from: string | null;
}

function useGeometry() {
  const ref = useRef<HTMLDivElement>(null);
  const [at, setAt] = useState(() => geometry(0));
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(([e]) => {
      const next = geometry(e?.contentRect.width ?? 0);
      setAt((now) =>
        now.columns === next.columns && now.width === next.width ? now : next,
      );
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, at] as const;
}

/** What a card in the stack for `from` shows and does; see `useCardEditor`. */
export type CardPropsFor = (
  card: DeckCard,
  from: string | null,
) => CardViewProps;

export function StacksView({
  categories,
  cards,
  printings,
  onDrop,
  cardProps,
}: {
  categories: readonly Category[];
  cards: readonly DeckCard[];
  printings: Printings;
  onDrop: OnDrop;
  /** Without it a card is only drawn: no menu, hotkeys or details. */
  cardProps?: CardPropsFor;
}) {
  const [ref, at] = useGeometry();
  const [dragging, setDragging] = useState<Dragging | null>(null);
  const [naming, setNaming] = useState<{
    drag: Dragging;
    secondary: boolean;
  } | null>(null);
  const nameOf = (c: DeckCard) => cardName(c, printings);
  const packed = packColumns(
    groupByCategory(categories, cards, nameOf),
    stackHeight(at),
    at.columns,
  );

  const drop = (to: DropTarget, secondary: boolean) => {
    if (dragging) onDrop(dragging.card, dragging.from, to, secondary);
    setDragging(null);
  };

  return (
    <>
      {dragging && (
        <DropStrip
          onDrop={(to, secondary) => {
            if (to !== null) return drop({ kind: "type", type: to }, secondary);
            setNaming({ drag: dragging, secondary });
            setDragging(null);
          }}
        />
      )}
      {naming && (
        <NewCategory
          onDone={(name) => {
            if (name)
              onDrop(
                naming.drag.card,
                naming.drag.from,
                { kind: "new", name },
                naming.secondary,
              );
            setNaming(null);
          }}
        />
      )}
      <div
        ref={ref}
        className={dragging ? "stacks dragging" : "stacks"}
        style={
          {
            "--card-w": `${at.width}px`,
            "--card-h": `${at.height}px`,
            "--peek": `${at.peek}px`,
            "--gap": `${GAP}px`,
          } as React.CSSProperties
        }
      >
        {packed.map((column, i) => (
          // Columns are positional: a resize repacks them, it does not move them.
          // biome-ignore lint/suspicious/noArrayIndexKey: see above
          <div className="stacks-column" key={i}>
            {column.map((group) => (
              <Stack
                key={group.category ?? UNCATEGORIZED}
                group={group}
                printings={printings}
                cardProps={cardProps}
                dragging={dragging}
                onDragStart={(card) =>
                  // Deferred: changing the DOM inside dragstart makes Chrome
                  // cancel the drag it has just begun.
                  setTimeout(() => setDragging({ card, from: group.category }))
                }
                onDragEnd={() => setDragging(null)}
                onDrop={(secondary) =>
                  group.category !== null &&
                  drop({ kind: "category", name: group.category }, secondary)
                }
              />
            ))}
          </div>
        ))}
      </div>
    </>
  );
}

/** Ctrl adds a category instead of moving, as in Archidekt. */
function useDropTarget(onDrop: (secondary: boolean) => void) {
  const [over, setOver] = useState<"move" | "secondary" | null>(null);
  return {
    over,
    handlers: {
      onDragOver(e: DragEvent) {
        e.preventDefault();
        e.dataTransfer.dropEffect = e.ctrlKey ? "copy" : "move";
        setOver(e.ctrlKey ? "secondary" : "move");
      },
      onDragLeave() {
        setOver(null);
      },
      onDrop(e: DragEvent) {
        e.preventDefault();
        setOver(null);
        onDrop(e.ctrlKey);
      },
    },
  };
}

/** Archidekt's mark on the commander category, which always comes first. */
function Crown() {
  return (
    <svg
      className="stack-crown"
      viewBox="0 0 24 24"
      role="img"
      aria-label="Commander"
    >
      <title>Commander</title>
      <path
        fill="currentColor"
        d="M2 7l5 4 5-7 5 7 5-4-2 12H4L2 7zm2 14h16v2H4v-2z"
      />
    </svg>
  );
}

function Stack({
  group,
  printings,
  cardProps,
  dragging,
  onDragStart,
  onDragEnd,
  onDrop,
}: {
  group: Group;
  printings: Printings;
  cardProps: CardPropsFor | undefined;
  dragging: Dragging | null;
  onDragStart: (card: DeckCard) => void;
  onDragEnd: () => void;
  onDrop: (secondary: boolean) => void;
}) {
  const target = useDropTarget(onDrop);
  const title = group.category ?? UNCATEGORIZED;
  // A card can be dropped on any category it is not being dragged out of;
  // Uncategorized is where cards are for want of one, not a place to put them.
  const droppable =
    dragging !== null &&
    group.category !== null &&
    dragging.from !== group.category;
  return (
    <section className="stack" data-category={group.category ?? undefined}>
      <header className="stack-header">
        <h2>
          {group.kind === "commander" && <Crown />}
          {title}
        </h2>
        <span className="stack-qty">
          {group.kind && group.kind !== "commander" && (
            <span className="stack-kind">{group.kind}</span>
          )}
          <span className="badge" title={`Qty: ${group.qty}`}>
            {group.qty}
          </span>
        </span>
      </header>
      <ol className="stack-cards">
        {group.cards.map((c) => (
          <li
            className="card"
            key={c.index}
            draggable
            onDragStart={(ev) => {
              ev.dataTransfer.effectAllowed = "copyMove";
              // Firefox starts no drag without data.
              ev.dataTransfer.setData("text/plain", cardName(c, printings));
              onDragStart(c);
            }}
            onDragEnd={onDragEnd}
          >
            <CardView
              {...(cardProps?.(c, group.category) ?? {
                name: cardName(c, printings),
                image: printings.get(printingKey(c.card))?.image,
                qty: c.qty,
                finish: c.finish,
              })}
            />
          </li>
        ))}
      </ol>
      {droppable && (
        <div
          className={`drop-target${target.over ? " over" : ""}`}
          {...target.handlers}
        >
          <span className="drop-plus">+</span>
          <span className="drop-name">{title}</span>
          <span className="drop-hint">
            {target.over === "secondary"
              ? "Add as another category"
              : "(Ctrl to add, not move)"}
          </span>
        </div>
      )}
    </section>
  );
}

/** Archidekt's strip of drop zones for places that are not a stack yet. */
function DropStrip({
  onDrop,
}: {
  onDrop: (to: "maybeboard" | "sideboard" | null, secondary: boolean) => void;
}) {
  return (
    <div className="drop-strip">
      <StripZone label="New category" onDrop={(s) => onDrop(null, s)} />
      <StripZone label="Maybeboard" onDrop={(s) => onDrop("maybeboard", s)} />
      <StripZone label="Sideboard" onDrop={(s) => onDrop("sideboard", s)} />
    </div>
  );
}

function StripZone({
  label,
  onDrop,
}: {
  label: string;
  onDrop: (secondary: boolean) => void;
}) {
  const target = useDropTarget(onDrop);
  return (
    <div
      className={`strip-zone${target.over ? " over" : ""}`}
      {...target.handlers}
    >
      + {label}
    </div>
  );
}

function NewCategory({ onDone }: { onDone: (name: string | null) => void }) {
  const input = useRef<HTMLInputElement>(null);
  useEffect(() => input.current?.focus(), []);
  return (
    <form
      className="new-category"
      onSubmit={(e) => {
        e.preventDefault();
        const name = new FormData(e.currentTarget).get("name");
        onDone(typeof name === "string" && name.trim() ? name.trim() : null);
      }}
    >
      <label>
        New category
        <input
          ref={input}
          name="name"
          onKeyDown={(e) => e.key === "Escape" && onDone(null)}
        />
      </label>
      <button type="submit">Add</button>
      <button type="button" onClick={() => onDone(null)}>
        Cancel
      </button>
    </form>
  );
}
