import { useEffect, useState } from "react";
import { PrintingsGrid } from "../card/PrintingsGrid";
import { FINISHES, printsByName } from "../card/prints";
import { usePrintingOptions } from "../card/usePrintingOptions";
import type { OwnedCard } from "../collection";
import type { Finish } from "../deck";
import { type Printing, printingId } from "../scryfall";
import { CloseIcon } from "../ui/icons";

/**
 * Which printing and finish some of a line's copies are, picked from every
 * printing as a picture so it can be matched against the card in hand: the
 * scanner's guess corrected, or the foil found in a stack. Nothing changes
 * until Apply, which makes it one edit.
 */
export function ReprintDialog({
  card,
  name,
  printing,
  onApply,
  onClose,
}: {
  card: OwnedCard;
  name: string;
  /** The line's printing as the page knows it, when Scryfall answered. */
  printing: Printing | undefined;
  /** Returns false when the edit was refused, which keeps the dialog open. */
  onApply: (
    qty: number,
    printing: { set: string; num: string } | null,
    finish: Finish,
  ) => boolean;
  onClose: () => void;
}) {
  const uri =
    printing?.prints ??
    (card.card.kind === "name" ? printsByName(card.card.name) : undefined);
  const options = usePrintingOptions(uri);
  const current = card.card.kind === "printing" ? printingId(card.card) : null;
  const [picked, setPicked] = useState<{ set: string; num: string } | null>(
    null,
  );
  const [finish, setFinish] = useState(card.finish);
  const [qty, setQty] = useState(card.qty);
  const pickedId = picked ? printingId(picked) : current;
  const changed =
    (picked !== null && printingId(picked) !== current) ||
    finish !== card.finish;
  const available =
    options.status === "done"
      ? options.printings.find((p) => printingId(p) === pickedId)?.finishes
      : undefined;

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    // biome-ignore lint/a11y/noStaticElementInteractions: the backdrop closes on a click, Escape does it by key
    // biome-ignore lint/a11y/useKeyWithClickEvents: Escape is handled on window
    <div
      className="details-backdrop"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div
        className="details-modal grid reprint-modal"
        role="dialog"
        aria-modal="true"
        aria-label={`Printing of ${name}`}
      >
        <header className="details-header">
          <h2>{name}</h2>
          <button
            type="button"
            className="details-close icon ghost"
            aria-label="Close"
            title="Close (Esc)"
            onClick={onClose}
          >
            <CloseIcon />
          </button>
        </header>
        <div className="reprint-bar">
          {card.qty > 1 && (
            <label className="reprint-qty">
              Change
              <input
                type="number"
                aria-label="How many copies to change"
                min={1}
                max={card.qty}
                value={qty}
                onChange={(e) => {
                  const n = Number(e.target.value);
                  if (Number.isInteger(n) && n >= 1 && n <= card.qty) setQty(n);
                }}
              />
              of {card.qty}
            </label>
          )}
          <fieldset className="segmented reprint-finish" aria-label="Finish">
            {FINISHES.map((f) => (
              <button
                key={f}
                type="button"
                aria-pressed={f === finish}
                disabled={
                  f !== finish &&
                  available !== undefined &&
                  !available.includes(f)
                }
                onClick={() => setFinish(f)}
              >
                {f}
              </button>
            ))}
          </fieldset>
          <button
            type="button"
            className="primary"
            disabled={!changed}
            onClick={() => {
              if (onApply(qty, picked, finish)) onClose();
            }}
          >
            Apply
          </button>
        </div>
        <div className="details-body">
          {options.status === "done" ? (
            <PrintingsGrid
              printings={options.printings}
              current={pickedId}
              onPick={(p) => {
                setPicked({ set: p.set, num: p.num });
                // A finish the printing never came in would be a copy no one owns.
                const [first] = p.finishes;
                if (first && !p.finishes.includes(finish)) setFinish(first);
              }}
            />
          ) : (
            <p className="details-note">
              {options.status === "error"
                ? options.message
                : options.status === "none"
                  ? "Scryfall does not know this card."
                  : "Asking Scryfall for printings…"}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}
