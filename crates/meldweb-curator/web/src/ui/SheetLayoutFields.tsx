import type { Paper, SheetLayout } from "../proxies";
import { perPage } from "../proxies";
import { messageOf } from "./Sheet";

const PAPERS: readonly { paper: Paper; label: string }[] = [
  { paper: "a4", label: "A4" },
  { paper: "letter", label: "Letter" },
  { paper: "a3", label: "A3" },
  { paper: "legal", label: "Legal" },
];

/**
 * Paper, gap, margin and cut marks: Phyrexian Metamorph's flags, which Mirage
 * Mockery's sheets take too, so proxies and custom cards print alike.
 */
export function SheetLayoutFields({
  layout,
  onChange,
}: {
  layout: SheetLayout;
  onChange: (layout: SheetLayout) => void;
}) {
  let fits: string;
  try {
    fits = `${perPage(layout)} cards to a page.`;
  } catch (e) {
    fits = messageOf(e);
  }
  return (
    <>
      <div className="proxy-layout">
        <label className="field">
          Paper
          <select
            value={layout.paper}
            onChange={(e) =>
              onChange({ ...layout, paper: e.target.value as Paper })
            }
          >
            {PAPERS.map((p) => (
              <option key={p.paper} value={p.paper}>
                {p.label}
              </option>
            ))}
          </select>
        </label>
        <label className="field">
          Gap (mm)
          <input
            type="number"
            min={0}
            step={0.5}
            value={layout.gapMm}
            onChange={(e) =>
              onChange({ ...layout, gapMm: Number(e.target.value) })
            }
          />
        </label>
        <label className="field">
          Margin (mm)
          <input
            type="number"
            min={0}
            step={0.5}
            value={layout.marginMm}
            onChange={(e) =>
              onChange({ ...layout, marginMm: Number(e.target.value) })
            }
          />
        </label>
        <label className="check">
          <input
            type="checkbox"
            checked={layout.cutMarks}
            onChange={(e) =>
              onChange({ ...layout, cutMarks: e.target.checked })
            }
          />
          Cut marks
        </label>
      </div>
      <p className="hint field-hint">{fits}</p>
    </>
  );
}
