import { useState } from "react";
import { besideCard, type CardFile, cardStem } from "../github/cards";
import { checkCard, sheetPdf } from "../mockery";
import { DEFAULT_LAYOUT, type SheetLayout } from "../proxies";
import type { Repo } from "../repo/api";
import { downloadPdf } from "../ui/download";
import { PrintIcon } from "../ui/icons";
import { messageOf, Sheet } from "../ui/Sheet";
import { SheetLayoutFields } from "../ui/SheetLayoutFields";
import { artFor } from "./useCardChecks";

/**
 * A sheet of custom cards to print and cut, as `mockery sheet` makes it.
 */
export function PrintCards({
  repo,
  cards,
}: {
  repo: Repo;
  cards: readonly CardFile[];
}) {
  const [open, setOpen] = useState(false);
  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        title="A PDF of custom cards to print and cut out"
      >
        <PrintIcon />
        Print sheet…
      </button>
      {open && (
        <PrintDialog repo={repo} cards={cards} onClose={() => setOpen(false)} />
      )}
    </>
  );
}

function PrintDialog({
  repo,
  cards,
  onClose,
}: {
  repo: Repo;
  cards: readonly CardFile[];
  onClose: () => void;
}) {
  const [qty, setQty] = useState<Record<string, number>>(() =>
    Object.fromEntries(cards.map((c) => [c.path, 1])),
  );
  const [layout, setLayout] = useState<SheetLayout>(DEFAULT_LAYOUT);
  const [step, setStep] = useState<string | null>(null);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);

  const make = async () => {
    setRefusal(null);
    setDone(null);
    try {
      const wanted = cards.filter((c) => (qty[c.path] ?? 0) > 0);
      if (wanted.length === 0) throw new Error("No cards to print.");
      setStep("Reading the cards…");
      const art = new Map<string, Uint8Array>();
      for (const c of wanted) {
        const checked = checkCard(c.text);
        if (checked.kind === "refused")
          throw new Error(`${c.path}: ${checked.message}`);
        for (const a of checked.arts) {
          const bytes = await artFor(repo, besideCard(c.path, a));
          if (!bytes)
            throw new Error(
              `${checked.name}'s art ${besideCard(c.path, a)} is not in the repo`,
            );
          art.set(a, bytes);
        }
      }
      setStep("Laying out…");
      // Let the step paint before Typst takes the thread.
      await new Promise((r) => setTimeout(r, 0));
      const pdf = sheetPdf(
        wanted.map((c) => ({ text: c.text, qty: qty[c.path] ?? 0 })),
        layout,
        art,
      );
      downloadPdf(pdf, "custom-cards.pdf");
      // A double-faced card prints both faces, and a meld pair four, each
      // a slot to cut.
      const n = wanted.reduce((s, c) => s + (qty[c.path] ?? 0), 0);
      const slots = wanted.reduce((s, c) => {
        const k = checkCard(c.text);
        return (
          s + (qty[c.path] ?? 0) * (k.kind === "card" ? k.images.length : 1)
        );
      }, 0);
      setDone(
        `${n} cards${slots > n ? ` (${slots} faces)` : ""}, ${(pdf.length / 1e3).toFixed(0)} KB.`,
      );
    } catch (e) {
      setRefusal(messageOf(e));
    } finally {
      setStep(null);
    }
  };

  return (
    <Sheet label="Print custom cards" onClose={onClose}>
      <form
        onSubmit={(e) => {
          e.preventDefault();
          void make();
        }}
      >
        <h2>Print custom cards</h2>
        <p className="hint">
          Drawn in vectors on Mirage Mockery's own frame, with the Fan Content
          Policy's notice in the margin.
        </p>
        <ul className="print-cards">
          {cards.map((c) => (
            <li key={c.path}>
              <label>
                <input
                  type="number"
                  min={0}
                  value={qty[c.path] ?? 0}
                  onChange={(e) =>
                    setQty({
                      ...qty,
                      [c.path]: Math.max(0, Number(e.target.value)),
                    })
                  }
                />
                {cardStem(c.path)}
              </label>
            </li>
          ))}
        </ul>
        <SheetLayoutFields layout={layout} onChange={setLayout} />
        {done && <p className="hint">{done}</p>}
        {refusal && (
          <p className="refusal" role="alert">
            {refusal}
          </p>
        )}
        <div className="dialog-buttons">
          <button type="button" onClick={onClose}>
            Close
          </button>
          <button type="submit" className="primary" disabled={step !== null}>
            {step ?? "Make PDF"}
          </button>
        </div>
      </form>
    </Sheet>
  );
}
