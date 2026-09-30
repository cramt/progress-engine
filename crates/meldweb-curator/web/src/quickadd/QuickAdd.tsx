import { useEffect, useId, useMemo, useRef, useState } from "react";
import type { Category } from "../deck";
import "./quickadd.css";
import {
  createAutocomplete,
  type Lookup,
  scryfallAutocomplete,
} from "./autocomplete";

/** Adds one copy of `name` to `category`; `null` is Automatic (or `anywhere`). */
export type AddByName = (name: string, category: string | null) => void;

/** True for Archidekt's quick add hotkey, `Ctrl+'`. */
export function isQuickAddKey(e: KeyboardEvent): boolean {
  return (e.ctrlKey || e.metaKey) && (e.key === "'" || e.code === "Quote");
}

/**
 * Archidekt's quick add: a box in the toolbar, focused with `Ctrl+'`, that
 * autocompletes card names from Scryfall and adds one copy of the name picked,
 * to the category its options name (Automatic, or any of the deck's).
 */
export function QuickAdd({
  categories,
  onAdd,
  lookup = scryfallAutocomplete,
  anywhere = "Automatic",
}: {
  categories: readonly Pick<Category, "name">[];
  onAdd: AddByName;
  lookup?: Lookup;
  /** What the `null` option is called: the collection's is Unsorted. */
  anywhere?: string;
}) {
  const input = useRef<HTMLInputElement>(null);
  const listId = useId();
  const [value, setValue] = useState("");
  const [names, setNames] = useState<string[]>([]);
  const [active, setActive] = useState(0);
  const [open, setOpen] = useState(false);
  const [category, setCategory] = useState<string | null>(null);
  const [added, setAdded] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Enter pressed before the names arrived picks the first one when they do.
  const pendingEnter = useRef<string | null>(null);

  const pickRef = useRef<(name: string) => void>(() => {});

  const auto = useMemo(
    () =>
      createAutocomplete({
        lookup,
        onResults: (query, found) => {
          setNames(found);
          setActive(0);
          setError(null);
          const first = found[0];
          if (pendingEnter.current === query && first) pickRef.current(first);
        },
        onError: (e) => setError(e instanceof Error ? e.message : String(e)),
      }),
    [lookup],
  );
  useEffect(() => auto.cancel, [auto]);

  pickRef.current = (name: string) => {
    onAdd(name, category);
    pendingEnter.current = null;
    auto.cancel();
    setValue("");
    setNames([]);
    setOpen(false);
    setAdded(name);
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (!isQuickAddKey(e)) return;
      e.preventDefault();
      input.current?.focus();
      input.current?.select();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (added === null) return;
    const t = setTimeout(() => setAdded(null), 2500);
    return () => clearTimeout(t);
  }, [added]);

  const pick = (name: string) => pickRef.current(name);
  const shown = open && names.length > 0;

  return (
    <div className="quick-add">
      <div className="quick-add-box">
        <input
          ref={input}
          type="search"
          role="combobox"
          aria-label="Quick add"
          aria-expanded={shown}
          aria-controls={listId}
          aria-autocomplete="list"
          {...(shown ? { "aria-activedescendant": `${listId}-${active}` } : {})}
          placeholder="Quick add (Ctrl+')"
          autoComplete="off"
          spellCheck={false}
          value={value}
          onChange={(e) => {
            setValue(e.target.value);
            setOpen(true);
            pendingEnter.current = null;
            auto.input(e.target.value);
          }}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          onKeyDown={(e) => {
            if (e.key === "ArrowDown" || e.key === "ArrowUp") {
              e.preventDefault();
              setOpen(true);
              const step = e.key === "ArrowDown" ? 1 : -1;
              setActive((a) =>
                names.length === 0
                  ? 0
                  : (a + step + names.length) % names.length,
              );
            } else if (e.key === "Enter") {
              e.preventDefault();
              const name = names[active];
              if (shown && name) pick(name);
              else if (value.trim().length >= 2)
                pendingEnter.current = value.trim();
            } else if (e.key === "Escape") {
              if (shown) setOpen(false);
              else input.current?.blur();
            }
          }}
        />
        {shown && (
          <div className="quick-add-list" id={listId} role="listbox">
            {names.map((name, i) => (
              <div
                key={name}
                id={`${listId}-${i}`}
                role="option"
                tabIndex={-1}
                aria-selected={i === active}
                className={i === active ? "active" : undefined}
                // Before the input's blur closes the list.
                onMouseDown={(e) => {
                  e.preventDefault();
                  pick(name);
                }}
                onMouseEnter={() => setActive(i)}
              >
                {name}
              </div>
            ))}
          </div>
        )}
      </div>
      <select
        aria-label="Quick add category"
        title="Where quick add puts a card"
        value={category ?? ""}
        onChange={(e) => setCategory(e.target.value || null)}
      >
        <option value="">{anywhere}</option>
        {categories.map((c) => (
          <option key={c.name} value={c.name}>
            {c.name}
          </option>
        ))}
      </select>
      <span className="quick-add-status" role="status">
        {error ?? (added ? `Added ${added}` : "")}
      </span>
    </div>
  );
}
