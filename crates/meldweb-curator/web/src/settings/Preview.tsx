import { useEffect, useId, useMemo, useState } from "react";
import { PinHeart } from "../card/PinHeart";
import type { PreferredOrder } from "../card/preference";
import type { PrintingOptions } from "../card/usePrintingOptions";
import {
  createAutocomplete,
  scryfallAutocomplete,
} from "../quickadd/autocomplete";
import { printingId } from "../scryfall";
import type { RuleText } from "./rules";

/** Cards whose printings pull the rules in different directions. */
export const SAMPLES = [
  "Forest",
  "Sol Ring",
  "Counterspell",
  "Heroic Intervention",
  "Lightning Bolt",
] as const;

/** How many printings show before "Show all", enough for two or three rows. */
const FIRST = 12;

/**
 * One card's printings, ranked by the rules being edited, as the details
 * modal will list them: what each rule does, shown before it is saved.
 */
export function Preview({
  card,
  onCard,
  options,
  ranked,
  skipped,
  lit,
}: {
  card: string;
  onCard: (name: string) => void;
  options: PrintingOptions;
  ranked: PreferredOrder | null;
  /** Rules left out because they do not parse yet. */
  skipped: number;
  /** The rule pointed at in the list: its printings stand out. */
  lit: RuleText | null;
}) {
  const [all, setAll] = useState(false);
  // A new card starts folded again.
  // biome-ignore lint/correctness/useExhaustiveDependencies: on a new card only
  useEffect(() => setAll(false), [card]);
  const list = ranked?.ranked ?? [];
  const shown = all ? list : list.slice(0, FIRST);
  const matches = (matched: readonly RuleText[]) =>
    lit !== null &&
    matched.some((r) => r.verb === lit.verb && r.query === lit.query);

  return (
    <aside className="settings-preview" aria-labelledby="settings-preview">
      <div className="settings-preview-head">
        <h2 id="settings-preview">Preview</h2>
        <CardPicker card={card} onCard={onCard} />
      </div>
      <div className="preview-samples">
        {SAMPLES.map((name) => (
          <button
            key={name}
            type="button"
            className="small"
            aria-pressed={name === card}
            onClick={() => onCard(name)}
          >
            {name}
          </button>
        ))}
      </div>
      {options.status === "loading" && (
        <p className="preview-note">Asking Scryfall for every {card}…</p>
      )}
      {options.status === "error" && (
        <p className="refusal" role="alert">
          Scryfall did not answer for {card}: {options.message}
        </p>
      )}
      {options.status === "done" && list.length === 0 && (
        <p className="preview-note">Scryfall has no card called {card}.</p>
      )}
      {ranked?.kind === "refused" && (
        <p className="refusal" role="alert">
          {ranked.message}
        </p>
      )}
      {list.length > 0 && (
        <>
          <p className="preview-note">
            {list.length} printing{list.length === 1 ? "" : "s"} of {card}, in
            the order you'll be offered them.
            {skipped > 0 &&
              ` ${skipped} rule${skipped === 1 ? " doesn't" : "s don't"} read yet and ${skipped === 1 ? "is" : "are"} left out.`}
          </p>
          <ol
            className={lit ? "preview-list lighting" : "preview-list"}
            aria-label={`${card}, best first`}
          >
            {shown.map(({ option: p, matched, pinned }, i) => (
              <li
                key={printingId(p)}
                className={matches(matched) ? "printing lit" : "printing"}
                title={`${p.setName} (${p.set.toUpperCase()}) #${p.num}`}
              >
                <span className="preview-rank">{i + 1}</span>
                {p.small || p.image ? (
                  <img
                    crossOrigin="anonymous"
                    src={p.image ?? p.small}
                    alt={`${p.name}, ${p.setName}`}
                    loading="lazy"
                  />
                ) : (
                  <span className="card-missing">{p.name}</span>
                )}
                <span className="printing-set">{p.setName}</span>
                <span className="printing-meta">
                  {p.set.toUpperCase()} #{p.num} · {p.released}
                </span>
                {pinned && (
                  <span className="printing-pinned">♥ Your {p.name}</span>
                )}
                {matched.length > 0 && (
                  <span className="printing-rules">
                    {matched.map((r) => (
                      <span
                        key={`${r.verb}:${r.query}`}
                        className={`printing-rule ${r.verb}`}
                      >
                        {r.verb === "prefer" ? "↑" : "↓"} {r.query}
                      </span>
                    ))}
                  </span>
                )}
                <PinHeart printing={p} />
              </li>
            ))}
          </ol>
          {list.length > FIRST && (
            <button
              type="button"
              className="ghost preview-more"
              onClick={() => setAll(!all)}
            >
              {all ? `Show the first ${FIRST}` : `Show all ${list.length}`}
            </button>
          )}
        </>
      )}
    </aside>
  );
}

/** A card name, with Scryfall's autocomplete; Enter or a pick previews it. */
function CardPicker({
  card,
  onCard,
}: {
  card: string;
  onCard: (name: string) => void;
}) {
  const listId = useId();
  const [value, setValue] = useState(card);
  const [names, setNames] = useState<string[]>([]);
  useEffect(() => setValue(card), [card]);
  const auto = useMemo(
    () =>
      createAutocomplete({
        lookup: scryfallAutocomplete,
        onResults: (_, found) => setNames(found),
      }),
    [],
  );
  useEffect(() => auto.cancel, [auto]);
  return (
    <form
      className="preview-picker"
      onSubmit={(e) => {
        e.preventDefault();
        const name = value.trim();
        if (name)
          onCard(
            names.find((n) => n.toLowerCase() === name.toLowerCase()) ?? name,
          );
      }}
    >
      <input
        type="search"
        list={listId}
        aria-label="Card to preview"
        placeholder="Any card"
        value={value}
        onChange={(e) => {
          const name = e.target.value;
          setValue(name);
          auto.input(name);
          // Picking from the list fills the box with an exact name.
          if (names.includes(name)) onCard(name);
        }}
      />
      <datalist id={listId}>
        {names.map((n) => (
          <option key={n} value={n} />
        ))}
      </datalist>
    </form>
  );
}
