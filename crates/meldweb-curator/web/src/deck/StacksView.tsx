import { type DragEvent, useEffect, useRef, useState } from "react";
import type { Entry } from "../decklist";
import { type Printings, printingKey } from "../scryfall";
import { type Group, groupByCategory, packColumns } from "./layout";

// Card geometry, in px. The peek is the name bar a stacked card leaves showing.
const CARD_W = 236;
const CARD_H = Math.round((CARD_W * 88) / 63);
const PEEK = Math.round(CARD_H * 0.112);
const HEADER = 48;
const GAP = 24;

const stackHeight = (g: Group) =>
  HEADER + (g.entries.length - 1) * PEEK + CARD_H + GAP;

export type OnDrop = (
  entry: Entry,
  from: string,
  to: string,
  secondary: boolean,
) => void;

/** The card being dragged, and the category it was dragged out of. */
interface Dragging {
  entry: Entry;
  from: string;
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
  entries,
  printings,
  onDrop,
}: {
  entries: readonly Entry[];
  printings: Printings;
  onDrop: OnDrop;
}) {
  const [ref, columns] = useColumnCount();
  const [dragging, setDragging] = useState<Dragging | null>(null);
  const [naming, setNaming] = useState<{
    drag: Dragging;
    secondary: boolean;
  } | null>(null);
  const packed = packColumns(groupByCategory(entries), stackHeight, columns);

  const drop = (to: string, secondary: boolean) => {
    if (dragging) onDrop(dragging.entry, dragging.from, to, secondary);
    setDragging(null);
  };

  return (
    <>
      {dragging && (
        <DropStrip
          onDrop={(to, secondary) => {
            if (to !== null) return drop(to, secondary);
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
                naming.drag.entry,
                naming.drag.from,
                name,
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
                key={group.name}
                group={group}
                printings={printings}
                dragging={dragging}
                onDragStart={(entry) =>
                  // Deferred: changing the DOM inside dragstart makes Chrome
                  // cancel the drag it has just begun.
                  setTimeout(() => setDragging({ entry, from: group.name }))
                }
                onDragEnd={() => setDragging(null)}
                onDrop={(secondary) => drop(group.name, secondary)}
              />
            ))}
          </div>
        ))}
      </div>
    </>
  );
}

/** Ctrl adds a secondary category instead of moving, as in Archidekt. */
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
  onDragStart: (entry: Entry) => void;
  onDragEnd: () => void;
  onDrop: (secondary: boolean) => void;
}) {
  const target = useDropTarget(onDrop);
  return (
    <section className="stack">
      <header className="stack-header">
        <h2>
          {group.commander && <span aria-hidden="true">♛ </span>}
          {group.name}
        </h2>
        <span className="stack-qty">Qty: {group.qty}</span>
      </header>
      <ol className="stack-cards">
        {group.entries.map((e) => (
          <li
            className="card"
            key={e.line}
            draggable
            onDragStart={(ev) => {
              ev.dataTransfer.effectAllowed = "copyMove";
              // Firefox starts no drag without data.
              ev.dataTransfer.setData("text/plain", e.name);
              onDragStart(e);
            }}
            onDragEnd={onDragEnd}
          >
            <Card entry={e} printings={printings} />
          </li>
        ))}
      </ol>
      {dragging && dragging.from !== group.name && (
        <div
          className={`drop-target${target.over ? " over" : ""}`}
          {...target.handlers}
        >
          <span className="drop-plus">+</span>
          <span className="drop-name">{group.name}</span>
          <span className="drop-hint">
            {target.over === "secondary"
              ? "Add as secondary"
              : "(Ctrl to add secondary)"}
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
  onDrop: (to: string | null, secondary: boolean) => void;
}) {
  return (
    <div className="drop-strip">
      <StripZone label="New category" onDrop={(s) => onDrop(null, s)} />
      <StripZone label="Maybeboard" onDrop={(s) => onDrop("Maybeboard", s)} />
      <StripZone label="Sideboard" onDrop={(s) => onDrop("Sideboard", s)} />
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

function Card({ entry, printings }: { entry: Entry; printings: Printings }) {
  const printing = printings.get(printingKey(entry));
  return (
    <>
      {printing ? (
        <img
          src={printing.image}
          alt={entry.name}
          loading="lazy"
          draggable={false}
        />
      ) : (
        <div className="card-missing">{entry.name}</div>
      )}
      <span className="card-qty">{entry.qty}</span>
    </>
  );
}
