import { useState } from "react";
import { CardMenu } from "../card/CardView";
import { type Card, exportDeck } from "../deck";
import type { Printings } from "../scryfall";
import { CopyIcon } from "../ui/icons";
import { printingNames } from "./archidektNames";
import { EXPORT_TARGETS } from "./exportTargets";

/**
 * Copy for: the whole deck on the clipboard as one tool imports it, a menu
 * entry per tool however alike two of their formats are, and `onCopied` to
 * say so: the button may sit in a menu that is closed by then. What the export refuses, such as a printing with no name yet,
 * goes to `onRefusal`.
 */
export function CopyDeck({
  text,
  cards,
  printings,
  onCopied,
  onRefusal,
}: {
  text: string;
  cards: readonly Card[];
  printings: Printings;
  onCopied: (label: string) => void;
  onRefusal: (message: string) => void;
}) {
  const [menuAt, setMenuAt] = useState<{ x: number; y: number } | null>(null);
  const copy = async (to: (typeof EXPORT_TARGETS)[number]) => {
    try {
      const exported = exportDeck(
        text,
        to.target,
        printingNames(cards, printings),
      );
      await navigator.clipboard.writeText(exported);
      onCopied(to.label);
    } catch (e) {
      onRefusal(e instanceof Error ? e.message : String(e));
    }
  };

  return (
    <>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={menuAt !== null}
        onClick={(e) => {
          const r = e.currentTarget.getBoundingClientRect();
          setMenuAt({ x: r.left, y: r.bottom + 4 });
        }}
        title="Copy the deck as another tool imports it"
      >
        <CopyIcon />
        Copy for…
      </button>
      {menuAt && (
        <CardMenu
          entries={EXPORT_TARGETS.map((to) => ({
            label: to.label,
            run: () => void copy(to),
          }))}
          x={menuAt.x}
          y={menuAt.y}
          onClose={() => setMenuAt(null)}
        />
      )}
    </>
  );
}
