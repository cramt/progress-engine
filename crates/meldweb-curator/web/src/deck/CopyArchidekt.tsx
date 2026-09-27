import { useEffect, useState } from "react";
import { type Card, exportArchidekt } from "../deck";
import type { Printings } from "../scryfall";
import { printingNames } from "./printings";

/**
 * Archidekt's *Copy as Archidekt*: the whole deck on the clipboard as the
 * text Archidekt imports, and a brief "Copied" to say so. What the export
 * refuses, such as a printing with no name yet, goes to `onRefusal`.
 */
export function CopyArchidekt({
  text,
  cards,
  printings,
  onRefusal,
}: {
  text: string;
  cards: readonly Card[];
  printings: Printings;
  onRefusal: (message: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 2000);
    return () => clearTimeout(t);
  }, [copied]);

  const copy = async () => {
    try {
      const archidekt = exportArchidekt(text, printingNames(cards, printings));
      await navigator.clipboard.writeText(archidekt);
      setCopied(true);
    } catch (e) {
      onRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <button
      type="button"
      onClick={() => void copy()}
      title="Copy the deck as Archidekt text"
    >
      {copied ? "Copied" : "Copy as Archidekt"}
    </button>
  );
}
