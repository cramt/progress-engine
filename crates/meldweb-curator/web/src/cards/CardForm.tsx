import { useRef } from "react";
import { ManaText } from "../card/ManaText";
import type { Form, FormCorner } from "../mockery";
import {
  addSymbol,
  CARD_TYPES,
  costSymbols,
  dropSymbol,
  genericOf,
  insertAt,
  SUPERTYPES,
  splitType,
  toggleColor,
  toggleType,
  withGeneric,
} from "./cardForm";

const COLOURS = ["W", "U", "B", "R", "G", "C", "X", "S"] as const;
const HYBRID = [
  "W/U",
  "U/B",
  "B/R",
  "R/G",
  "G/W",
  "W/B",
  "U/R",
  "B/G",
  "R/W",
  "G/U",
];
const PHYREXIAN = ["W/P", "U/P", "B/P", "R/P", "G/P"];
const RULES_SYMBOLS = ["T", "Q", "W", "U", "B", "R", "G", "C", "X", "E"];
const CORNERS: readonly { kind: FormCorner["kind"]; label: string }[] = [
  { kind: "none", label: "None" },
  { kind: "pt", label: "P/T" },
  { kind: "loyalty", label: "Loyalty" },
  { kind: "defense", label: "Defense" },
];

/**
 * A custom card as a form: the same file as the TOML view, each field with
 * buttons for what it holds. Every change is written into the file by Mirage
 * Mockery, so the TOML view shows it as it is.
 */
export function CardForm({
  form,
  onChange,
}: {
  form: Form;
  onChange: (form: Form) => void;
}) {
  const set = <K extends keyof Form>(key: K, value: Form[K]) =>
    onChange({ ...form, [key]: value });
  const { types } = splitType(form.typeLine);

  return (
    <div className="card-form">
      <label className="field">
        Name
        <input
          value={form.name}
          onChange={(e) => set("name", e.target.value)}
        />
      </label>

      <CostField cost={form.cost} onChange={(c) => set("cost", c)} />

      <fieldset className="field">
        <legend>Type</legend>
        <div className="card-form-chips">
          {[...SUPERTYPES, ...CARD_TYPES].map((t) => (
            <button
              key={t}
              type="button"
              aria-pressed={types.includes(t)}
              onClick={() => set("typeLine", toggleType(form.typeLine, t))}
            >
              {t}
            </button>
          ))}
        </div>
        <input
          aria-label="Type line"
          placeholder="Creature — Homunculus"
          value={form.typeLine}
          onChange={(e) => set("typeLine", e.target.value)}
        />
      </fieldset>

      <fieldset className="field">
        <legend>Colour indicator</legend>
        <div className="card-form-chips card-form-symbols">
          {(["W", "U", "B", "R", "G"] as const).map((c) => (
            <button
              key={c}
              type="button"
              title={`{${c}}`}
              aria-pressed={form.colorIndicator.includes(c)}
              onClick={() =>
                set("colorIndicator", toggleColor(form.colorIndicator, c))
              }
            >
              <ManaText text={`{${c}}`} />
            </button>
          ))}
        </div>
        <span className="hint">
          Colours a card with no mana cost, like a back face
        </span>
      </fieldset>

      <RulesField rules={form.rules} onChange={(r) => set("rules", r)} />

      <CornerField corner={form.corner} onChange={(c) => set("corner", c)} />

      <label className="field">
        Flavor text
        <textarea
          rows={2}
          value={form.flavor}
          onChange={(e) => set("flavor", e.target.value)}
        />
      </label>

      <div className="card-form-row">
        <label className="field">
          Art
          <input
            placeholder="art/my-card.png, beside the card file"
            value={form.art}
            onChange={(e) => set("art", e.target.value)}
          />
        </label>
        <label className="field">
          Artist
          <input
            value={form.artist}
            onChange={(e) => set("artist", e.target.value)}
          />
        </label>
      </div>
    </div>
  );
}

function CostField({
  cost,
  onChange,
}: {
  cost: string;
  onChange: (cost: string) => void;
}) {
  const generic = genericOf(cost);
  const symbols = costSymbols(cost);
  return (
    <fieldset className="field">
      <legend>Mana cost</legend>
      <div className="card-form-row">
        <div className="stepper">
          <button
            type="button"
            aria-label="Less generic mana"
            disabled={generic === 0}
            onClick={() => onChange(withGeneric(cost, generic - 1))}
          >
            −
          </button>
          <span>{generic}</span>
          <button
            type="button"
            aria-label="More generic mana"
            onClick={() => onChange(withGeneric(cost, generic + 1))}
          >
            +
          </button>
        </div>
        <SymbolButtons
          symbols={COLOURS}
          onPick={(s) => onChange(addSymbol(cost, s))}
        />
      </div>
      <details className="card-form-more">
        <summary>Hybrid and Phyrexian</summary>
        <SymbolButtons
          symbols={[...HYBRID, ...PHYREXIAN]}
          onPick={(s) => onChange(addSymbol(cost, s))}
        />
      </details>
      <div className="card-form-row">
        <div className="card-form-cost">
          {symbols.length === 0 ? (
            <span className="hint">No cost</span>
          ) : (
            symbols.map((s, i) => (
              <button
                // A cost can hold the same symbol twice; its place is its key.
                // biome-ignore lint/suspicious/noArrayIndexKey: see above
                key={i}
                type="button"
                title={`Drop {${s}}`}
                aria-label={`Drop {${s}}`}
                onClick={() => onChange(dropSymbol(cost, i))}
              >
                <ManaText text={`{${s}}`} />
              </button>
            ))
          )}
        </div>
        <input
          aria-label="Mana cost as text"
          className="card-form-cost-text"
          placeholder="{2}{U}"
          value={cost}
          onChange={(e) => onChange(e.target.value)}
        />
      </div>
    </fieldset>
  );
}

function SymbolButtons({
  symbols,
  onPick,
}: {
  symbols: readonly string[];
  onPick: (symbol: string) => void;
}) {
  return (
    <div className="card-form-symbols">
      {symbols.map((s) => (
        <button
          key={s}
          type="button"
          title={`{${s}}`}
          onClick={() => onPick(s)}
        >
          <ManaText text={`{${s}}`} />
        </button>
      ))}
    </div>
  );
}

function RulesField({
  rules,
  onChange,
}: {
  rules: string;
  onChange: (rules: string) => void;
}) {
  const area = useRef<HTMLTextAreaElement>(null);
  const insert = (text: string) => {
    const el = area.current;
    const start = el?.selectionStart ?? rules.length;
    const end = el?.selectionEnd ?? rules.length;
    const next = insertAt(rules, text, start, end);
    onChange(next.text);
    requestAnimationFrame(() => {
      el?.focus();
      el?.setSelectionRange(next.caret, next.caret);
    });
  };
  return (
    <div className="field">
      <label htmlFor="card-form-rules">Rules text</label>
      <SymbolButtons symbols={RULES_SYMBOLS} onPick={(s) => insert(`{${s}}`)} />
      <textarea
        id="card-form-rules"
        ref={area}
        rows={5}
        placeholder="One ability a line, as Oracle text writes it"
        value={rules}
        onChange={(e) => onChange(e.target.value)}
      />
    </div>
  );
}

function CornerField({
  corner,
  onChange,
}: {
  corner: FormCorner;
  onChange: (corner: FormCorner) => void;
}) {
  const pick = (kind: FormCorner["kind"]): FormCorner => {
    switch (kind) {
      case "none":
        return { kind };
      case "pt":
        return { kind, power: "", toughness: "" };
      case "loyalty":
      case "defense":
        return { kind, value: "" };
    }
  };
  return (
    <fieldset className="field">
      <legend>Corner</legend>
      <div className="card-form-row">
        <div className="segmented">
          {CORNERS.map((c) => (
            <button
              key={c.kind}
              type="button"
              aria-pressed={corner.kind === c.kind}
              onClick={() => corner.kind !== c.kind && onChange(pick(c.kind))}
            >
              {c.label}
            </button>
          ))}
        </div>
        {corner.kind === "pt" && (
          <span className="card-form-pt">
            <input
              aria-label="Power"
              value={corner.power}
              onChange={(e) => onChange({ ...corner, power: e.target.value })}
            />
            /
            <input
              aria-label="Toughness"
              value={corner.toughness}
              onChange={(e) =>
                onChange({ ...corner, toughness: e.target.value })
              }
            />
          </span>
        )}
        {(corner.kind === "loyalty" || corner.kind === "defense") && (
          <input
            aria-label={corner.kind === "loyalty" ? "Loyalty" : "Defense"}
            className="card-form-corner-value"
            value={corner.value}
            onChange={(e) => onChange({ ...corner, value: e.target.value })}
          />
        )}
      </div>
    </fieldset>
  );
}
