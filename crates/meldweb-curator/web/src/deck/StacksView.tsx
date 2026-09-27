import { useEffect, useRef, useState } from "react";
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
}: {
  entries: readonly Entry[];
  printings: Printings;
}) {
  const [ref, columns] = useColumnCount();
  const packed = packColumns(groupByCategory(entries), stackHeight, columns);
  return (
    <div
      ref={ref}
      className="stacks"
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
            <Stack key={group.name} group={group} printings={printings} />
          ))}
        </div>
      ))}
    </div>
  );
}

function Stack({ group, printings }: { group: Group; printings: Printings }) {
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
          <li className="card" key={`${e.name}|${e.set}|${e.num}`}>
            <Card entry={e} printings={printings} />
          </li>
        ))}
      </ol>
    </section>
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
