import { type DragEvent, useEffect, useRef, useState } from "react";
import type { Category, Card as DeckCard } from "../deck";
import { type Printings, printingKey } from "../scryfall";
import {
  type Group,
  groupByCategory,
  packColumns,
  UNCATEGORIZED,
} from "./layout";

// Card geometry, in px. The peek is the name bar a stacked card leaves showing.
const CARD_W = 236;
const CARD_H = Math.round((CARD_W * 88) / 63);
const PEEK = Math.round(CARD_H * 0.112);
const HEADER = 48;
const GAP = 24;

const stackHeight = (g: Group) =>
  HEADER + (g.cards.length - 1) * PEEK + CARD_H + GAP;

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

function useColumnCount() {
  const ref = useRef<HTMLDivElement>(null);
  const [columns, setColumns] = useState(1);
  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const observer = new ResizeObserver(([e]) => {
      const width = e?.contentRect.width ?? 0;
      setColumns(Math.max(1, Math.floor((width + GAP) / (CARD_W + GAP))));
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, []);
  return [ref, columns] as const;
}

export function StacksView({
  categories,
  cards,
  printings,
  onDrop,
}: {
  categories: readonly Category[];
  cards: readonly DeckCard[];
  printings: Printings;
  onDrop: OnDrop;
}) {
  const [ref, columns] = useColumnCount();
  const [dragging, setDragging] = useState<Dragging | null>(null);
  const [naming, setNaming] = useState<{
    drag: Dragging;
    secondary: boolean;
  } | null>(null);
  const nameOf = (c: DeckCard) => cardName(c, printings);
  const packed = packColumns(
    groupByCategory(categories, cards, nameOf),
    stackHeight,
    columns,
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
            "--card-w": `${CARD_W}px`,
            "--card-h": `${CARD_H}px`,
            "--peek": `${PEEK}px`,
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

function cardName(card: DeckCard, printings: Printings): string {
  if (card.card.kind === "name") return card.card.name;
  return (
    printings.get(printingKey(card.card))?.name ??
    `${card.card.set}/${card.card.num}`
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

const KIND_LABEL: Partial<Record<string, string>> = {
  commander: "♛ ",
};

function Stack({
  group,
  printings,
  dragging,
  onDragStart,
  onDragEnd,
  onDrop,
}: {
  group: Group;
  printings: Printings;
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
    <section className="stack">
      <header className="stack-header">
        <h2>
          {group.kind && (
            <span aria-hidden="true">{KIND_LABEL[group.kind] ?? ""}</span>
          )}
          {title}
        </h2>
        <span className="stack-qty">
          Qty: {group.qty}
          {group.kind && group.kind !== "commander" && (
            <span className="stack-kind"> · {group.kind}</span>
          )}
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
            <Card card={c} printings={printings} />
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

function Card({ card, printings }: { card: DeckCard; printings: Printings }) {
  const printing = printings.get(printingKey(card.card));
  const name = cardName(card, printings);
  return (
    <>
      {printing ? (
        <img src={printing.image} alt={name} loading="lazy" draggable={false} />
      ) : (
        <div className="card-missing">{name}</div>
      )}
      <span className="card-qty">{card.qty}</span>
    </>
  );
}
