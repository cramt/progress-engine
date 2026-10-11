import { type ReactNode, useState } from "react";
import type { Card, CardRef, Finish } from "../deck";
import { type Printings, printingKey } from "../scryfall";
import { CheckIcon, CopyIcon } from "../ui/icons";
import { Preview } from "./CostView";
import { printingLabel } from "./sourcing";
import "./cost.css";
import "./supply.css";

/**
 * A card by name with its printing beside it, and its picture on hover: the
 * copy's own printing where it has one, else the deck line's.
 */
export function CardCell({
  name,
  card,
  finish,
  line,
  printings,
  note,
}: {
  name: string;
  card: CardRef;
  finish: Finish;
  /** The deck line the copy is for, whose picture stands in for a copy by name. */
  line: Card | undefined;
  printings: Printings;
  note?: ReactNode;
}) {
  const image =
    printings.get(printingKey(card))?.image ??
    (line && printings.get(printingKey(line.card))?.image);
  const label = printingLabel(card, finish);
  return (
    <td className="cost-name cost-hover">
      <span className="cost-thumb" aria-hidden="true">
        {image && (
          <img crossOrigin="anonymous" src={image} alt="" loading="lazy" />
        )}
      </span>
      <span className="cost-title">{name}</span>
      {label && <span className="muted cost-printing">{label}</span>}
      {note && <span className="supply-note">{note}</span>}
      <Preview image={image} />
    </td>
  );
}

/** A section of a view: a heading, how many copies, and a table. */
export function SupplySection({
  title,
  count,
  hint,
  actions,
  children,
}: {
  title: string;
  count: number;
  hint?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section className="supply-section" aria-label={title}>
      <header className="supply-header">
        <h2>{title}</h2>
        <span className="badge" title="Copies">
          {count}
        </span>
        {hint && <span className="muted supply-hint">{hint}</span>}
        {actions && <span className="supply-actions">{actions}</span>}
      </header>
      <table className="cost-table supply-table">{children}</table>
    </section>
  );
}

/** Copies `text` and says so. */
export function CopyText({
  text,
  label,
  title,
}: {
  text: string;
  label: string;
  title: string;
}) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="small"
      title={title}
      onClick={() =>
        void navigator.clipboard.writeText(text).then(() => setCopied(true))
      }
    >
      {copied ? <CheckIcon /> : <CopyIcon />}
      {copied ? "Copied" : label}
    </button>
  );
}
